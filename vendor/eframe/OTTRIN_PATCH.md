# Native viewport accessibility initialization

Origin: eframe 0.33.3, egui commit `44cdd653e2317d300fb8a6c9c36b03f23991e803`.

Initialize AccessKit when each native viewport receives its winit state, including
secondary Settings and Inspector windows. Create a new window hidden until its
adapter is installed, then apply the requested visibility. Root first-paint
visibility remains unchanged. Share the existing event-loop proxy across native
viewport creation in both Glow and WGPU. No separate accessibility service,
parallel renderer or application-level bridge is introduced.

Native baseline evidence is under `.hermes/evidence/assistive-modals-current/`.
