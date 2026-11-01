//! Own only the chooser's native relationship; never disable or mutate the caller.
use egui::Context;
use ottrin_core::picker::PickerParentWindow;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

pub(crate) struct PickerParent {
    gone: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl PickerParent {
    pub(crate) fn attach(
        cc: &eframe::CreationContext<'_>,
        value: &str,
    ) -> Result<Option<Self>, String> {
        let parent = PickerParentWindow::parse(value)?;
        if parent == PickerParentWindow::None {
            return Ok(None);
        }
        let child = cc.window_handle().map_err(|e| e.to_string())?.as_raw();
        #[cfg(target_os = "linux")]
        match (parent, child) {
            (PickerParentWindow::X11(parent), RawWindowHandle::Xlib(child)) => {
                return Self::x11(
                    parent,
                    u32::try_from(child.window).map_err(|e| e.to_string())?,
                    &cc.egui_ctx,
                )
                .map(Some);
            }
            (PickerParentWindow::X11(parent), RawWindowHandle::Xcb(child)) => {
                return Self::x11(parent, child.window.get(), &cc.egui_ctx).map(Some);
            }
            (PickerParentWindow::Wayland(parent), RawWindowHandle::Wayland(child)) => {
                use raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
                if let RawDisplayHandle::Wayland(display) =
                    cc.display_handle().map_err(|e| e.to_string())?.as_raw()
                {
                    return Self::wayland(
                        &parent,
                        display.display.as_ptr(),
                        child.surface.as_ptr(),
                        &cc.egui_ctx,
                    )
                    .map(Some);
                }
            }
            _ => {}
        }
        #[cfg(target_os = "windows")]
        if let (PickerParentWindow::Windows(parent), RawWindowHandle::Win32(child)) =
            (parent, child)
        {
            return Self::windows(parent, child.hwnd.get(), &cc.egui_ctx).map(Some);
        }
        Err("The picker parent identifier does not match the active window system".into())
    }
    fn watch(
        ctx: &Context,
        mut check: impl FnMut() -> bool + Send + 'static,
    ) -> Result<Self, String> {
        let gone = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_gone = gone.clone();
        let worker_stop = stop.clone();
        let ctx = ctx.clone();
        let worker = std::thread::Builder::new()
            .name("picker-parent".into())
            .spawn(move || {
                while !worker_stop.load(Ordering::Acquire) {
                    if !check() {
                        worker_gone.store(true, Ordering::Release);
                        ctx.request_repaint();
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            gone,
            stop,
            worker: Some(worker),
        })
    }
    pub(crate) fn gone(&self) -> bool {
        self.gone.load(Ordering::Acquire)
    }

    #[cfg(target_os = "linux")]
    fn x11(parent: u32, child: u32, ctx: &Context) -> Result<Self, String> {
        use x11rb::{
            connection::Connection,
            protocol::{
                Event,
                xproto::{AtomEnum, ChangeWindowAttributesAux, ConnectionExt, EventMask, PropMode},
            },
            wrapper::ConnectionExt as _,
        };
        let result = (|| -> Result<Self, Box<dyn std::error::Error>> {
            let (conn, _) = x11rb::connect(None)?;
            if parent == child || conn.setup().roots.iter().any(|s| s.root == parent) {
                return Err("The picker parent must be a separate application window".into());
            }
            conn.change_window_attributes(
                parent,
                &ChangeWindowAttributesAux::new().event_mask(EventMask::STRUCTURE_NOTIFY),
            )?
            .check()?;
            conn.get_geometry(parent)?.reply()?;
            conn.change_property32(
                PropMode::REPLACE,
                child,
                AtomEnum::WM_TRANSIENT_FOR,
                AtomEnum::WINDOW,
                &[parent],
            )?
            .check()?;
            let atom = |name: &[u8]| -> Result<u32, Box<dyn std::error::Error>> {
                Ok(conn.intern_atom(false, name)?.reply()?.atom)
            };
            conn.change_property32(
                PropMode::REPLACE,
                child,
                atom(b"_NET_WM_WINDOW_TYPE")?,
                AtomEnum::ATOM,
                &[atom(b"_NET_WM_WINDOW_TYPE_DIALOG")?],
            )?
            .check()?;
            conn.flush()?;
            Ok(Self::watch(ctx, move || {
                loop {
                    match conn.poll_for_event() {
                        Ok(Some(Event::DestroyNotify(event))) if event.window == parent => {
                            return false;
                        }
                        Ok(Some(_)) => {}
                        Ok(None) => return true,
                        Err(_) => return false,
                    }
                }
            })?)
        })();
        result.map_err(|e| format!("Could not attach picker to its X11 parent: {e}"))
    }

    #[cfg(target_os = "windows")]
    fn windows(parent: usize, child: isize, ctx: &Context) -> Result<Self, String> {
        use windows_sys::Win32::{
            Foundation::{GetLastError, SetLastError},
            UI::WindowsAndMessaging::{
                GWLP_HWNDPARENT, GetWindowThreadProcessId, IsWindow, SetWindowLongPtrW,
            },
        };
        let owner = parent as *mut std::ffi::c_void;
        let mut pid = 0;
        // SAFETY: Win32 validates these external HWND values; no pointer is dereferenced here.
        let tid = unsafe { GetWindowThreadProcessId(owner, &mut pid) };
        if tid == 0 || parent == child as usize || unsafe { IsWindow(owner) } == 0 {
            return Err("The picker parent window no longer exists".into());
        }
        unsafe {
            SetLastError(0);
        }
        let previous = unsafe { SetWindowLongPtrW(child as _, GWLP_HWNDPARENT, parent as isize) };
        let error = unsafe { GetLastError() };
        if previous == 0 && error != 0 {
            return Err(format!(
                "Could not attach picker to its Windows owner: {}",
                std::io::Error::from_raw_os_error(error as i32)
            ));
        }
        Self::watch(ctx, move || {
            let mut current_pid = 0;
            let current_tid = unsafe { GetWindowThreadProcessId(parent as _, &mut current_pid) };
            current_tid == tid && current_pid == pid && unsafe { IsWindow(parent as _) } != 0
        })
    }

    #[cfg(target_os = "linux")]
    fn wayland(
        parent: &str,
        display: *mut std::ffi::c_void,
        surface: *mut std::ffi::c_void,
        ctx: &Context,
    ) -> Result<Self, String> {
        wayland::attach(parent, display, surface, ctx)
    }
}
impl Drop for PickerParent {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take()
            && worker.join().is_err()
        {
            eprintln!("ottrin: picker parent watcher failed");
        }
    }
}

#[cfg(target_os = "linux")]
mod wayland {
    use super::*;
    use wayland_client::{
        Connection, Dispatch, Proxy, QueueHandle,
        protocol::{wl_registry, wl_surface},
    };
    use wayland_protocols::xdg::foreign::zv2::client::{zxdg_imported_v2, zxdg_importer_v2};
    #[derive(Default)]
    struct State {
        importer: Option<zxdg_importer_v2::ZxdgImporterV2>,
        gone: bool,
    }
    impl Dispatch<wl_registry::WlRegistry, ()> for State {
        fn event(
            state: &mut Self,
            registry: &wl_registry::WlRegistry,
            event: wl_registry::Event,
            _: &(),
            _: &Connection,
            qh: &QueueHandle<Self>,
        ) {
            if let wl_registry::Event::Global {
                name, interface, ..
            } = event
                && interface == "zxdg_importer_v2"
            {
                state.importer = Some(registry.bind(name, 1, qh, ()));
            }
        }
    }
    impl Dispatch<zxdg_importer_v2::ZxdgImporterV2, ()> for State {
        fn event(
            _: &mut Self,
            _: &zxdg_importer_v2::ZxdgImporterV2,
            _: zxdg_importer_v2::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }
    impl Dispatch<zxdg_imported_v2::ZxdgImportedV2, ()> for State {
        fn event(
            state: &mut Self,
            _: &zxdg_imported_v2::ZxdgImportedV2,
            event: zxdg_imported_v2::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            if let zxdg_imported_v2::Event::Destroyed = event {
                state.gone = true;
            }
        }
    }
    struct Link {
        connection: Connection,
        queue: wayland_client::EventQueue<State>,
        state: State,
        _registry: wl_registry::WlRegistry,
        imported: zxdg_imported_v2::ZxdgImportedV2,
    }
    impl Drop for Link {
        fn drop(&mut self) {
            self.imported.destroy();
            if let Some(importer) = self.state.importer.take() {
                importer.destroy();
            }
            if let Err(error) = self.connection.flush() {
                eprintln!("ottrin: picker parent cleanup: {error}");
            }
        }
    }
    pub(super) fn attach(
        parent: &str,
        display: *mut std::ffi::c_void,
        surface: *mut std::ffi::c_void,
        ctx: &Context,
    ) -> Result<PickerParent, String> {
        let result = (|| -> Result<PickerParent, Box<dyn std::error::Error>> {
            // SAFETY: eframe supplies live handles. This backend borrows the display,
            // owns only its private queue, and is dropped with the App before winit's window.
            let backend = unsafe {
                wayland_backend::sys::client::Backend::from_foreign_display(display.cast())
            };
            let connection = Connection::from_backend(backend);
            let mut queue = connection.new_event_queue();
            let qh = queue.handle();
            let registry = connection.display().get_registry(&qh, ());
            let mut state = State::default();
            queue.roundtrip(&mut state)?;
            let importer = state
                .importer
                .as_ref()
                .ok_or("This compositor does not support foreign picker parents")?;
            let imported = importer.import_toplevel(parent.to_owned(), &qh, ());
            queue.roundtrip(&mut state)?;
            if state.gone {
                return Err("The picker parent window no longer exists".into());
            }
            // Borrowing a proxy does not take ownership of the caller-supplied wl_surface.
            let id = unsafe {
                wayland_backend::sys::client::ObjectId::from_ptr(
                    wl_surface::WlSurface::interface(),
                    surface.cast(),
                )?
            };
            let child = wl_surface::WlSurface::from_id(&connection, id)?;
            imported.set_parent_of(&child);
            connection.flush()?;
            let mut link = Link {
                connection,
                queue,
                state,
                _registry: registry,
                imported,
            };
            Ok(PickerParent::watch(ctx, move || {
                // winit owns socket reads; dispatch only our private queue. Never steal
                // its events, disconnect its display, or destroy its surface.
                link.queue.dispatch_pending(&mut link.state).is_ok() && !link.state.gone
            })?)
        })();
        result.map_err(|e| format!("Could not attach picker to its Wayland parent: {e}"))
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, GW_OWNER, GWL_STYLE, GetWindow, GetWindowLongPtrW,
        IsWindow, WS_DISABLED, WS_OVERLAPPEDWINDOW,
    };
    struct Window(windows_sys::Win32::Foundation::HWND);
    impl Window {
        fn new() -> Self {
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            let handle = unsafe {
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    class.as_ptr(),
                    WS_OVERLAPPEDWINDOW,
                    0,
                    0,
                    100,
                    100,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                )
            };
            assert!(!handle.is_null(), "{}", std::io::Error::last_os_error());
            Self(handle)
        }
    }
    impl Drop for Window {
        fn drop(&mut self) {
            if unsafe { IsWindow(self.0) } != 0 {
                assert_ne!(unsafe { DestroyWindow(self.0) }, 0);
            }
        }
    }
    #[test]
    fn native_owner_is_attached_without_disabling_caller_and_loss_is_observed() {
        let parent = Window::new();
        let child = Window::new();
        let ctx = Context::default();
        assert_eq!(
            unsafe { GetWindowLongPtrW(parent.0, GWL_STYLE) } & WS_DISABLED as isize,
            0
        );
        let link = PickerParent::windows(parent.0 as usize, child.0 as isize, &ctx).unwrap();
        assert_eq!(unsafe { GetWindow(child.0, GW_OWNER) }, parent.0);
        assert_eq!(
            unsafe { GetWindowLongPtrW(parent.0, GWL_STYLE) } & WS_DISABLED as isize,
            0
        );
        assert!(!link.gone());
        drop(parent);
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !link.gone() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        drop(link);
        assert_eq!(unsafe { IsWindow(child.0) }, 0);
    }
    #[test]
    fn absent_native_owner_is_rejected_without_mutating_child() {
        let child = Window::new();
        assert!(PickerParent::windows(0, child.0 as isize, &Context::default()).is_err());
        assert!(unsafe { GetWindow(child.0, GW_OWNER) }.is_null());
        assert_ne!(unsafe { IsWindow(child.0) }, 0);
    }
}
