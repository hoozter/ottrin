# Current Linux package candidates

Published at the stable `dist/packages/` paths:

- `Ottrin-0.1.0-x86_64.AppImage` (existing accepted appearance candidate).
- `Ottrin-0.1.0-linux-x86_64-portable.tar.gz`.
- `ottrin_0.1.0_amd64.deb`.
- `ottrin-0.1.0-1.x86_64.rpm`.

The three additional candidates use clean package source revision `0b8edcbb8094b68fd2863d5b5a4b9865385361cc`. Runtime source is unchanged from accepted binary revision `436924961e4f0d964bebf85e844919007f1ced09`; all five reusable release binaries were compared against the exact accepted AppImage before packaging. Portable and DEB executable hashes match. RPM applies its standard stripping step; its executable code and read-only data sections match. The DEB now explicitly declares the document renderer's Fontconfig dependency. Native packages use distribution runtime libraries; portable media playback uses system GStreamer rather than bundled codecs.

All three extracted packages displayed the main window on a private Xvfb display with separate temporary HOME/XDG profiles and a private DBus session. Each reported a ready index and watcher for its one-file fixture. Screenshots, launch logs and detailed receipts are in `.hermes/evidence/linux-package-candidates/`; package checksums and provenance are adjacent to the deliverables. `LINUX_PACKAGES_ACCEPTANCE.json` binds these checks to exact artifact hashes.

This verifies extracted-package startup on the current Ubuntu host, not installation, upgrade, uninstall or dependency resolution on a clean distribution. The RPM's generated metadata requires glibc 2.39; older RPM distributions are not accepted. Audible media, physical storage, privileged integration and native compositor acceptance remain open. Private-session portal/systemd warnings are retained in the logs. The initial test profile used an overlong runtime path and failed startup; launch acceptance uses short temporary runtime paths. User settings and the running host index service were preserved. Task-owned apps, index daemons, buses, displays, portal helpers and temporary profiles were stopped and removed.

## Native libc admission

DEB packaging derives `libc6 (>= VERSION)` from the highest versioned GLIBC import across all five staged executables. Invalid ELF payloads or an unavailable ABI requirement fail before replacing the current artifact. RPM retains its native automatic versioned ELF dependency generation. This prevents package managers admitting a build whose executable loader requires a newer libc than the installed system; it does not broaden supported-distribution acceptance.
