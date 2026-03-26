# Packaging Strategy

## Principles
1. End-user installs must be standalone — no `cargo` required at runtime.
2. Prefer simple, familiar installers for each OS.

## Linux
1. Primary: AppImage for widest compatibility.
2. Secondary: native packages (`.deb`, `.rpm`) for distro integration.
3. Build expectations:
- static assets bundled with binary
- config stored under XDG config path
- no root requirement for runtime
4. Integrated privilege management packaging:
- install `ottrin-priv-helper` as root-owned executable (recommended: `/usr/libexec/ottrin-priv-helper`)
- install polkit action file at `/usr/share/polkit-1/actions/org.ottrin.filesystem.policy`
- helper discovery order at runtime:
  - explicit override: `--helper-path=/absolute/path/to/ottrin-priv-helper`
  - environment: `OTTRIN_PRIV_HELPER=/absolute/path/to/ottrin-priv-helper`
  - auto-discovery: app directory and standard install paths (`/usr/libexec/ottrin`, `/usr/lib/ottrin`, `/usr/libexec`)
  - fallback: `ottrin-priv-helper` via `PATH`
- include policy template from `packaging/linux/org.ottrin.filesystem.policy`

## Windows
1. Portable `.zip` build for early adopters and testing.
2. `.exe` installer for simple install flow.
3. MSI installer for stable releases.
4. MSIX for Microsoft Store (later).
3. Build expectations:
- install scope: per-user by default
- config stored under `%APPDATA%/Ottrin/`
- uninstaller removes binaries but preserves user config unless explicitly chosen
4. Integrated privilege management packaging:
- ship `ottrin-priv-helper.exe` alongside `ottrin-app.exe` (or in a known install directory)
- helper discovery order at runtime:
  - explicit override: `--helper-path=C:\path\to\ottrin-priv-helper.exe`
  - environment: `OTTRIN_PRIV_HELPER=C:\path\to\ottrin-priv-helper.exe`
  - auto-discovery: app directory (plus `helpers`/`bin` subfolders)
  - fallback: `ottrin-priv-helper.exe` via `PATH`
- UAC elevation path uses PowerShell `Start-Process -Verb RunAs`
- sign helper binary for smoother SmartScreen/UAC trust behavior
