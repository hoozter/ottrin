# Ottrin Development

## Workspace

Ottrin is a Rust workspace using egui/eframe.

| Crate | Purpose |
| --- | --- |
| `ottrin-app` | Native eframe entrypoint |
| `ottrin-ui` | egui application shell and UI workflows |
| `ottrin-core` | Domain models, settings, commands, and semantic file classification |
| `ottrin-platform` | OS integrations, file operations, trash, and privileged helper execution |
| `ottrin-search` | Indexed search, filesystem watching, and query fallback logic |
| `ottrin-preview` | Preview classification and loading |

The supported Rust toolchain is pinned in `rust-toolchain.toml`.

## Build And Verify

```sh
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release
```

Linux development dependencies on Debian/Ubuntu:

```sh
sudo apt-get install -y \
    libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
    libxkbcommon-dev libssl-dev libglib2.0-dev
```

Linux native Open With support requires GIO `>= 2.56`. The Linux portable and
AppImage builders bundle the required GIO/GLib runtime closure; this development
package is not an end-user package dependency for those artifacts.

Its `BUNDLED_RUNTIME_MANIFEST.txt` is an **unsigned build record**: its hashes
bind the shipped runtime bytes and copied notices for integrity checks and
diagnosis, but they do not authenticate a final artifact. Someone able to
rewrite an artifact can rewrite this record too. Final-artifact authenticity
requires a future signed artifact or attestation and is not claimed here.

The Linux runtime-closure builder is intentionally Debian-family specific: it
requires `dpkg-query` to record package/version and copyright provenance and
fails closed when that evidence is unavailable. Its supported artifact ABI is
the system dynamic loader, glibc base ABI, and base C++ runtime (`libgcc_s` and
`libstdc++`), including standard glibc split components such as `libresolv`,
`libutil`, and `libanl` when needed. Those components remain host-provided;
Ottrin does not claim broad Linux portability.

GIO uses host desktop-entry and XDG MIME data by design. The artifacts do not
claim to bundle GIO modules or schemas. Packaging has an artifact-level
content-type smoke with empty XDG data, but clean/minimal desktop acceptance is
a separate release gate.

## Packaging

End-user packages must run without Cargo.

Build all release artifacts available on the current machine:

```sh
./build-all.sh
```

On Linux this builds the Linux portable tarball and AppImage. It also builds the Windows portable zip when the Windows GNU target is already installed. To require the Windows artifact:

```sh
rustup target add x86_64-pc-windows-gnu
./build-all.sh --with-windows
```

Linux portable:

```sh
./packaging/linux/package-portable.sh
```

Linux AppImage:

```sh
./packaging/linux/package-appimage.sh
```

Windows portable from Linux with the GNU target:

```sh
./packaging/windows/package-portable.sh
```

Windows portable from PowerShell:

```powershell
.\packaging\windows\package-portable.ps1
```

Windows installer from an extracted portable payload (requires NSIS):

```powershell
.\packaging\windows\package-installer.ps1 -Stage .\dist\Ottrin-0.1.0-windows-x64
```

Linux AppImage is the first public Linux package target. Native `.deb` and `.rpm` packages are deferred until after the first portable/AppImage smoke-test cycle. Windows ships a portable zip and an NSIS per-user installer candidate; MSI work remains.

## Privileged Helper Model

Privileged operations are per action, never a global root/admin mode.

- The UI requests a typed privileged file operation through `ottrin-platform`.
- The platform crate serializes a request to `ottrin-priv-helper`.
- Linux launches the helper through `pkexec` and a polkit policy.
- Windows launches the helper through the UAC flow.
- The helper validates an allowlist of supported filesystem operations and returns a typed result.

Linux packaging should install:

```sh
sudo install -m 0755 ottrin-priv-helper /usr/libexec/ottrin-priv-helper
sudo install -m 0644 packaging/linux/org.ottrin.filesystem.policy /usr/share/polkit-1/actions/
```

Runtime helper discovery checks explicit override, environment variable, app directory, standard install paths, and finally `PATH`.

## QA Checklist

### Linux

- Navigate Miller/List/Grid, tabs, bookmarks, and address bar.
- Search first index, restart from DB, rebuild, current-folder search, content search, and result actions.
- Create, rename, copy, trash delete, and permanent delete; verify same-filesystem Linux Move with Skip/Rename/Overwrite/Cancel and explicit dispositions. Cross-filesystem and Windows Move must continue to reject without mutation unless a separately accepted protocol is implemented and proven for that platform and mode.
- Tandem activation, pinning, side switching, and pane-to-pane copy.
- Quick Inspector for text, image, PDF text/metadata, DOCX, XLSX/XLSM, PPTX, archive, folder, and generic fallback files.
- Privileged helper smoke with helper and policy installed.
- Helper-missing and policy-misconfigured error paths.

### Windows

- Launch the portable zip on a real Windows desktop.
- Navigate, search, run file operations, and validate Trash/Recycling Bin behavior.
- Validate Quick Inspector and Tandem View.
- Validate UAC allow/cancel paths and helper-missing behavior.
- Validate background index updater scheduling/removal.

## Performance Discipline

- Keep filesystem operations off the UI thread.
- Use background workers for directory listing, preview loading, search, trash loading, and long operations.
- Cap expensive UI rendering for very large folders.
- Preserve release-mode search benchmark coverage.
- Avoid logging full directory contents in release builds.

## Release Gate

Before a public v1 release:

- Run `./scripts/release-preflight.sh` for the repeatable, non-publishing local
  gate. Use `--source-only` before artifacts exist, or
  `--require-optional-tools` on a fully provisioned release workstation.
- `cargo fmt --check`, workspace check/test, strict Clippy, and release build pass.
- Linux and Windows CI are green.
- Linux AppImage and portable tarball are checksum-verified and smoke-tested from clean artifacts.
- Windows portable zip is launched and smoke-tested on real Windows.
- Linux privileged helper/polkit smoke test passes.
- Windows UAC/helper smoke test passes.
- `README.md`, `docs/USER_GUIDE.md`, `docs/ROADMAP.md`, and `docs/RELEASE_NOTES.md` accurately describe the shipped artifacts and known limitations.

## Theme Source Provenance

Theme presets are derived from upstream palettes where possible:

- Breeze: KDE Breeze color scheme files.
- Adwaita: GNOME libadwaita palette and stylesheet colors.
- Windows 11: Microsoft Fluent UI color tokens.
- Solarized: Ethan Schoonover's Solarized palette.
- Nord: official Nord palette.
- G33k: custom terminal-inspired palette.

When updating a theme preset, keep the mapping deterministic and document any intentional deviation in code comments near the preset definition.
