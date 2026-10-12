# Current Linux package candidates

Published at the stable `dist/packages/` paths:

- `Ottrin-0.1.0-x86_64.AppImage` (existing accepted appearance candidate).
- `Ottrin-0.1.0-linux-x86_64-portable.tar.gz`.
- `ottrin_0.1.0_amd64.deb`.
- `ottrin-0.1.0-1.x86_64.rpm`.

The AppImage and portable/RPM payloads currently use clean revision `6e8cef0`. The DEB and RPM use package revision `221e02b`, with the same accepted `6e8cef0` executable code and corrected libc/authentication dependencies. Exact checksums and provenance are adjacent to each artifact; `LINUX_PACKAGES_ACCEPTANCE.json` distinguishes current native package verification from prior package runtime evidence.

The earlier `6e8cef0` DEB passed fresh Ubuntu 24.04.5 VM installation, standard-user text-preview launch/clean exit, same-version reinstall/removal with preferences preserved, and real systemd user-service installation/status/restart/removal. Receipt: `DEB_INSTALL_ACCEPTANCE.json`. The current DEB/RPM have payload and generated dependency verification in `NATIVE_AUTH_RUNTIME_ACCEPTANCE.json`, including extracted DEB CLI launch; their clean-system installation has not been repeated.

Portable PDFium placement and exact archive PDF preview are accepted. Native packages use distribution runtime libraries, and portable media playback uses system GStreamer. Cross-version upgrade, fresh RPM installation, full desktop integration, helper authentication and reboot/logout service persistence remain open. Historical receipts apply only to their recorded artifact hashes.

## Native libc admission

DEB packaging derives `libc6 (>= VERSION)` from the highest versioned GLIBC import across all five staged executables. Invalid ELF payloads or an unavailable ABI requirement fail before replacing the current artifact. RPM explicitly requires the same payload-derived versioned GLIBC symbol in addition to native automatic dependency generation. This prevents package managers admitting a build whose executable loader requires a newer libc than the installed system; it does not broaden supported-distribution acceptance.

Native package installation must also provide administrator authentication: DEB requires `pkexec` and `polkitd`; RPM requires `polkit` and the `/usr/bin/pkexec` provider. The desktop session supplies its authentication agent. Shipping only the helper and action policy is insufficient on a clean system. Authentication prompt/cancel/error behavior remains a separate acceptance gate.
