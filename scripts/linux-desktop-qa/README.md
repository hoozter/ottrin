# Private Linux desktop QA

`session.py` supplies `OwnedDesktop(evidence_directory, scale="1")` for actual
Ottrin GUI checks. It starts a private Xvfb display, session bus and KWin process
with disposable HOME/XDG state through `scripts/test-workspace.py`; Cargo and
Rustup continue to use the existing shared caches. It never controls the host
session. The harness selects X11 consistently for EGL, GTK, Qt and Winit,
removing inherited Wayland display/socket values. The caller must declare its fixtures, impose a finite lease, check app
PID/start time/display ownership before input and record the exact binary hash.

Use the desktop as a context manager. Teardown stops owned process groups and
profile children, releases classified unused FUSE mounts, and only then removes
the profile. An unclassified mount, active consumer or other teardown failure
retains the profile and writes its location/error to `TEARDOWN.json`. Never delete
that profile until the failed resource teardown has been resolved. Cancellation,
startup failure and normal exit all follow the same cleanup path.

Run-specific requests, input guards, screenshots, assertions and compact receipts
belong in that run's evidence directory. A source GUI result does not accept a
packaged artifact or establish cold first-paint/resource performance.
