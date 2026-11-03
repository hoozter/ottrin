//! Apply application-requested corners to the exact native viewport being painted.

use egui::{Context, Id, ViewportId};
use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
use windows_sys::Win32::Graphics::Gdi::{
    CreateRectRgn, CreateRoundRectRgn, DeleteObject, EqualRgn, GetWindowRgn, HRGN, SetWindowRgn,
};

fn request_key(viewport: ViewportId) -> Id {
    Id::new(("eframe_window_corner_radius", viewport))
}

/// Set the current Windows viewport's corner radius in logical egui points.
///
/// Both native renderers apply the request to their own window before painting.
/// Maximized and fullscreen windows remain rectangular. This does not enable
/// framebuffer transparency; Windows owns hit testing for the resulting region.
pub fn set_window_corner_radius(ctx: &Context, radius: f32) {
    let physical_radius = if radius.is_finite() {
        radius.max(0.0) * ctx.pixels_per_point()
    } else {
        0.0
    };
    let key = request_key(ctx.viewport_id());
    ctx.data_mut(|data| {
        data.insert_temp(key, physical_radius);
    });
}

pub(crate) fn apply(ctx: &Context, viewport: ViewportId, window: &winit::window::Window) {
    let Some(radius) = ctx.data(|data| data.get_temp::<f32>(request_key(viewport))) else {
        return;
    };
    if let Err(error) = apply_region(window, radius) {
        log::error!("Cannot apply native corners to viewport {viewport:?}: {error}");
    }
}

struct Region(HRGN);

impl Region {
    fn new(handle: HRGN) -> std::io::Result<Self> {
        if handle.is_null() {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(Self(handle))
        }
    }
}

impl Drop for Region {
    #[expect(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: this region remains owned by this guard. Successful transfer
        // to Windows explicitly consumes the guard below.
        if unsafe { DeleteObject(self.0) } == 0 {
            log::error!(
                "Cannot release native corner region: {}",
                std::io::Error::last_os_error()
            );
        }
    }
}

#[expect(unsafe_code)]
fn apply_region(window: &winit::window::Window, radius: f32) -> std::io::Result<()> {
    let handle = window.window_handle().map_err(std::io::Error::other)?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Err(std::io::Error::other(
            "Windows viewport has no Win32 handle",
        ));
    };
    let size = window.outer_size();
    let width = i32::try_from(size.width).map_err(std::io::Error::other)?;
    let height = i32::try_from(size.height).map_err(std::io::Error::other)?;
    if width == 0 || height == 0 || window.is_minimized() == Some(true) {
        return Ok(());
    }
    let radius = if window.is_maximized() || window.fullscreen().is_some() {
        0
    } else {
        radius.round().clamp(0.0, (width.min(height) / 2) as f32) as i32
    };
    // SAFETY: coordinates are bounded by this live window's physical dimensions.
    // Each returned region has a single guard until ownership is transferred.
    let desired = Region::new(unsafe {
        if radius == 0 {
            CreateRectRgn(0, 0, width, height)
        } else {
            CreateRoundRectRgn(0, 0, width + 1, height + 1, radius * 2, radius * 2)
        }
    })?;
    // SAFETY: allocate an owned destination for the native region query.
    let current = Region::new(unsafe { CreateRectRgn(0, 0, 0, 0) })?;
    let hwnd = handle.hwnd.get() as _;
    // SAFETY: the backend holds this native window alive throughout painting;
    // no title matching, foreground lookup or foreign window handle is involved.
    // No explicit region is a valid initial state (GetWindowRgn returns zero).
    if unsafe { GetWindowRgn(hwnd, current.0) } != 0
        && unsafe { EqualRgn(current.0, desired.0) } != 0
    {
        return Ok(());
    }
    // SAFETY: SetWindowRgn consumes the desired region only on success. Comparing
    // first avoids its resize/repaint messages when the native shape is unchanged.
    if unsafe { SetWindowRgn(hwnd, desired.0, 1) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    std::mem::forget(desired); // Windows now owns and eventually destroys HRGN.
    Ok(())
}
