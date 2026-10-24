# Ottrin

**Navigate with clarity.**

A fast, keyboard-first file manager built in Rust with [egui](https://github.com/emilk/egui). Miller columns, blazing-fast indexed search, rich file previews, and a full theme system — no Electron, no web views.

[![CI](https://github.com/hoozter/ottrin/actions/workflows/ci.yml/badge.svg)](https://github.com/hoozter/ottrin/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/hoozter/ottrin)](https://github.com/hoozter/ottrin/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.94.0-orange.svg)](https://www.rust-lang.org)

---

## Release Artifacts

When a verified release is published, its artifacts will be available from
[GitHub Releases](https://github.com/hoozter/ottrin/releases). The current
v0.1.0 source tree remains a release candidate; no exact-current-commit
artifact is claimed as published or accepted. See the
[Release Checklist](docs/RELEASE_CHECKLIST.md) for outstanding gates.

The intended first release formats are Linux AppImage, portable tarball, DEB
and RPM, and Windows portable zip, per-user installer and per-user MSI. macOS
implementation and acceptance are deferred until the Linux/Windows roadmap is
complete. Windows MSI packages have been built and tested
natively for normal installation, upgrade, repair and removal. Failed-transaction
MSI registration rollback still fails native tests and blocks acceptance; see the
[roadmap](docs/ROADMAP.md) for exact artifact scopes. Native macOS construction
and platform acceptance remain pending.

The latest locally verified Linux build is kept at
`dist/packages/Ottrin-0.1.0-x86_64.AppImage`, with its checksum and source
provenance beside it. Bash packaging replaces the previous output only after
building its replacement successfully; it does not retain release archives.
Packaging stages and desktop QA profiles are disposable and removed when their
owning operation ends. Windows portable staging uses one managed
`windows-payload/` folder, replacing its previous contents. Installer and MSI packagers consume it; the full build
entry point removes it after both finish.

### Linux

File-operation provenance requires Linux 6.8 or newer. Ottrin uses unique mount
identities to reject a filesystem that changes between selection and execution.

```sh
chmod +x Ottrin-*.AppImage
./Ottrin-*.AppImage
```

For system integration (polkit privilege helper), install the helper and policy
from the portable archive at the paths below. For the AppImage, extract it with
`--appimage-extract` and use the corresponding files under `squashfs-root/usr/`.

```sh
sudo install -D -m 0755 ./bin/ottrin-priv-helper /usr/libexec/ottrin/ottrin-priv-helper
sudo install -m 0644 packaging/linux/org.ottrin.filesystem.policy \
    /usr/share/polkit-1/actions/
```

### Windows

Run the per-user installer, or unzip the portable package and run:

```powershell
.\bin\ottrin.exe
```

Normal user-owned file operations do not require elevation. Denied operations use the bundled helper through the Windows UAC flow.

Windows packaging is designed to use WGPU when Automatic finds no compatibility
signal and to select a checksum-pinned Mesa 26.1.3 OpenGL compatibility runtime
for detected remote sessions and recognized virtual/problem adapters. Settings
offers Automatic, Native, and OpenGL compatibility, while
`ottrin.exe --renderer opengl` (or `OTTRIN_RENDERER=opengl`) is the diagnostic
override. Exact release artifacts must still verify the app-local
`opengl32.dll`, `libgallium_wgl.dll`, EGL/GLES, DXIL, and complete notices before
this compatibility path is accepted.

### macOS

The macOS package source produces `Ottrin-*-macos-*.zip`, containing
`Ottrin.app`. It is not yet a published or accepted artifact: native launch,
Finder behavior, quarantine/Gatekeeper handling, and production signing remain
release gates on macOS.

---

## Features

- **Miller columns** — browse parent, current, and preview pane side-by-side
- **Smooth column navigation** — horizontal auto-scroll keeps focus visible while preserving path context
- **Resizable columns** — drag separators; widths persist while you navigate
- **List and Grid views** — switch instantly with a single click
- **Smart sidebar** — info, drop folder, and search panels in a compact, toggleable activity strip
- **Bookmarks bar** — one-click shortcuts to your most-used folders (right-click to add/remove)
- **Integrated command frame** — run `mkdir`, `touch`, `mv`, `cp`, `rm`, `chmod`, `ln -s` and more without leaving the app; opens on any keypress
- **Keyboard first** — arrow keys, Enter, Backspace, and Tab navigate the Miller view
- **Tabs** — multiple locations open at once, full navigation history per tab
- **Theme system** — built-in presets (Ottrin, Breeze, Adwaita, Windows 11, Solarized, Nord, G33k) with live editing and custom save/load
- **Semantic file styling** — file and folder types are classified and styled consistently across views
- **Drop Folder workflow** — quick copy destination with recents and pinned entries
- **Integrated privilege management** — denied operations can retry via helper flow
- **Global search** — indexed search with scope switching and per-result actions
- **Preview system** — text and images in-app, bounded multi-page PDF previews with Previous/Next/page selection, bounded SVG visuals admitted by source, XML, path-work, semantic reuse, filter-work, dimension, and pixel limits, bounded XLSX/XLSM and ODS structured worksheet previews, a bounded native DOCX structure view, and a clearly labeled PPTX extracted-text fallback, plus rich archive/media metadata (Space bar). The DOCX view preserves paragraphs, styled runs, alignment cues, and bordered tables without claiming exact pagination or Office fidelity; PDF and Office previews retain extracted-text and metadata fallbacks. On Linux, EPS can use separately installed Ghostscript through bounded non-networked isolation; Ottrin does not bundle it and retains metadata/external-open fallback when unavailable.
- **Coherent command line** — `ottrin search`/`-s`, `ottrin index` (`update`, read-only `status`, atomic `rebuild`), and full Quick Inspector `ottrin preview`/`-p`

---

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| Arrow keys | Navigate between files |
| Enter | Open directory / launch file |
| Backspace | Go to parent directory |
| Ctrl+F | Open search panel |
| Space | Preview file |
| Esc | Close preview / command frame |
| F5 | Refresh current directory |
| Ctrl+C / Ctrl+X | Copy / Cut |
| Ctrl+V | Paste |
| Delete | Move to trash |
| Shift+Delete | Permanently delete |
| F2 | Rename |
| Any printable key | Open command frame |

---

## Build from Source

Requires Rust 1.94.0. CI and release workflows pin this exact toolchain for Linux, Windows, and macOS builds.

```sh
git clone https://github.com/hoozter/ottrin.git
cd ottrin
cargo run -p ottrin-app
```

Build local release packages into `dist/packages/`:

```sh
./build-all.sh
```

Linux dependencies:

```sh
sudo apt-get install -y \
    libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
    libxkbcommon-dev libssl-dev libgstreamer1.0-dev \
    libgstreamer-plugins-base1.0-dev
```

---

## Workspace

| Crate | Purpose |
|-------|---------|
| `ottrin-app` | Native entrypoint (eframe) |
| `ottrin-ui` | egui application shell and all views |
| `ottrin-core` | Domain models, commands, settings |
| `ottrin-platform` | OS integrations (trash, shell actions, privileged helper flow) |
| `ottrin-search` | Indexed search, watcher sync, and fallback querying |
| `ottrin-preview` | Preview request/response pipeline |

---

## Status

v0.1.0 is in release hardening. Linux AppImage, portable, `.deb`, and `.rpm` package sources plus Windows portable/installer sources are the current release path. See [docs/USER_GUIDE.md](docs/USER_GUIDE.md), [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md), and [docs/ROADMAP.md](docs/ROADMAP.md).

## License

MIT

## Author

Made by [hoozter](https://hoozter.com) — [github.com/hoozter/ottrin](https://github.com/hoozter/ottrin)
