# Packaging Strategy

## Linux
1. Primary: AppImage for widest compatibility.
2. Secondary: native packages (`.deb`, `.rpm`) for distro integration.
3. Build expectations:
- static assets bundled with binary
- config stored under XDG config path
- no root requirement for runtime

## Windows
1. Portable `.zip` build for early adopters and testing.
2. MSI installer for stable releases.
3. Build expectations:
- install scope: per-user by default
- config stored under `%APPDATA%/Ottrin/`
- uninstaller removes binaries but preserves user config unless explicitly chosen
