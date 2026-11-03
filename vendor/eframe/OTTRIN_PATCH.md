# Native viewport accessibility initialization

Origin: eframe 0.33.3, egui commit `44cdd653e2317d300fb8a6c9c36b03f23991e803`.

Initialize AccessKit when each native viewport receives its winit state, including
secondary Settings and Inspector windows. Create a new window hidden until its
adapter is installed, then apply the requested visibility. Root first-paint
visibility remains unchanged. Share the existing event-loop proxy across native
viewport creation in both Glow and WGPU. No separate accessibility service,
parallel renderer or application-level bridge is introduced.

Native baseline evidence is under `.hermes/evidence/assistive-modals-current/`.

# Native Windows viewport corners

The application requests the current viewport radius in egui points through
`set_window_corner_radius`. Both Glow and WGPU apply that request before painting
their exact owned winit window, including immediate secondary viewports. Windows
uses a native GDI window region instead of the unavailable opaque-framebuffer
alpha mask. Radius changes follow egui scaling; maximized/fullscreen windows
remain rectangular. The current native region is compared before changing it,
avoiding repeated window-position/repaint messages. Region guards retain ownership
on failure and transfer it only after successful `SetWindowRgn`.

No foreground/title-based window lookup, second renderer or background service
is introduced. The pre-fix installed regression evidence is in
`.hermes/evidence/windows-window-radius-current/`; corrected native acceptance
and package delivery remain separate gates.
