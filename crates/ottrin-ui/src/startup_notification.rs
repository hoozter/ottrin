//! Apply desktop activation before revealing a new native window.
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StartupNotification {
    pub x11: Option<String>,
    pub wayland: Option<String>,
}
impl StartupNotification {
    pub fn pending(&self) -> bool {
        self.x11.is_some() || self.wayland.is_some()
    }
    pub fn apply(&self, cc: &eframe::CreationContext<'_>) {
        if !self.pending() {
            return;
        }
        let result = (|| -> Result<(), String> {
            match cc.window_handle().map_err(|e| e.to_string())?.as_raw() {
                RawWindowHandle::Xlib(window) => self.x11.as_deref().map_or(Ok(()), |token| {
                    x11(
                        u32::try_from(window.window).map_err(|e| e.to_string())?,
                        token,
                    )
                }),
                RawWindowHandle::Xcb(window) => self
                    .x11
                    .as_deref()
                    .map_or(Ok(()), |token| x11(window.window.get(), token)),
                RawWindowHandle::Wayland(window) => {
                    use raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
                    if let (Some(token), RawDisplayHandle::Wayland(display)) = (
                        self.wayland.as_deref(),
                        cc.display_handle().map_err(|e| e.to_string())?.as_raw(),
                    ) {
                        wayland::activate(
                            display.display.as_ptr(),
                            window.surface.as_ptr(),
                            token,
                        )?;
                    }
                    Ok(())
                }
                _ => Ok(()),
            }
        })();
        if let Err(error) = result {
            eprintln!("ottrin: desktop startup notification: {error}");
        }
        // Optional activation failure must never leave an otherwise usable window hidden.
        cc.egui_ctx
            .send_viewport_cmd(egui::ViewportCommand::Visible(true));
    }
}
fn x11(window: u32, token: &str) -> Result<(), String> {
    use x11rb::{
        connection::Connection,
        protocol::xproto::{AtomEnum, ClientMessageEvent, ConnectionExt, EventMask, PropMode},
        wrapper::ConnectionExt as _,
    };
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let (conn, screen) = x11rb::connect(None)?;
        let atom = |name: &[u8]| -> Result<u32, Box<dyn std::error::Error>> {
            Ok(conn.intern_atom(false, name)?.reply()?.atom)
        };
        conn.change_property8(
            PropMode::REPLACE,
            window,
            atom(b"_NET_STARTUP_ID")?,
            AtomEnum::STRING,
            token.as_bytes(),
        )?
        .check()?;
        let escaped = token.replace('\\', "\\\\").replace('"', "\\\"");
        let message = format!("remove: ID=\"{escaped}\"\0");
        let begin = atom(b"_NET_STARTUP_INFO_BEGIN")?;
        let more = atom(b"_NET_STARTUP_INFO")?;
        for (index, chunk) in message.as_bytes().chunks(20).enumerate() {
            let mut bytes = [0; 20];
            bytes[..chunk.len()].copy_from_slice(chunk);
            conn.send_event(
                false,
                conn.setup().roots[screen].root,
                EventMask::PROPERTY_CHANGE,
                ClientMessageEvent::new(8, window, if index == 0 { begin } else { more }, bytes),
            )?
            .check()?;
        }
        conn.flush()?;
        Ok(())
    })();
    result.map_err(|e| e.to_string())
}
mod wayland {
    use wayland_client::{
        Connection, Dispatch, Proxy, QueueHandle,
        protocol::{wl_registry, wl_surface},
    };
    use wayland_protocols::xdg::activation::v1::client::xdg_activation_v1;
    #[derive(Default)]
    struct State {
        activation: Option<xdg_activation_v1::XdgActivationV1>,
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
                && interface == "xdg_activation_v1"
            {
                state.activation = Some(registry.bind(name, 1, qh, ()));
            }
        }
    }
    impl Dispatch<xdg_activation_v1::XdgActivationV1, ()> for State {
        fn event(
            _: &mut Self,
            _: &xdg_activation_v1::XdgActivationV1,
            _: xdg_activation_v1::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }
    pub(super) fn activate(
        display: *mut std::ffi::c_void,
        surface: *mut std::ffi::c_void,
        token: &str,
    ) -> Result<(), String> {
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            // SAFETY: live eframe handles are borrowed only for this call. Its display/surface
            // remain owned by winit; only our registry, activation proxy and queue are destroyed.
            let backend = unsafe {
                wayland_backend::sys::client::Backend::from_foreign_display(display.cast())
            };
            let connection = Connection::from_backend(backend);
            let mut queue = connection.new_event_queue();
            let qh = queue.handle();
            let registry = connection.display().get_registry(&qh, ());
            let mut state = State::default();
            queue.roundtrip(&mut state)?;
            let activation = state
                .activation
                .take()
                .ok_or("This compositor does not provide xdg_activation_v1")?;
            // SAFETY: the raw surface belongs to the same live borrowed display.
            let id = unsafe {
                wayland_backend::sys::client::ObjectId::from_ptr(
                    wl_surface::WlSurface::interface(),
                    surface.cast(),
                )?
            };
            let surface = wl_surface::WlSurface::from_id(&connection, id)?;
            activation.activate(token.to_owned(), &surface);
            activation.destroy();
            connection.flush()?;
            drop(registry);
            Ok(())
        })();
        result.map_err(|e| e.to_string())
    }
}
