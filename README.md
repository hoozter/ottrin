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

The intended first release formats are Linux AppImage and portable tarball,
plus Windows portable zip and per-user installer. Windows MSI and macOS
packages remain planned work.

### Linux

```sh
chmod +x Ottrin-*.AppImage
./Ottrin-*.AppImage
```

For system integration (polkit privilege helper):

```sh
sudo cp ottrin-priv-helper /usr/libexec/ottrin-priv-helper
sudo cp packaging/linux/org.ottrin.filesystem.policy \
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
- **Preview system** — text and images in-app, bounded multi-page PDF previews with Previous/Next/page selection, bounded SVG visuals admitted by source, XML, path-work, semantic reuse, filter-work, dimension, and pixel limits, bounded XLSX/XLSM and ODS structured worksheet previews, and clearly labeled DOCX/PPTX extracted-text fallbacks, plus rich archive/media metadata (Space bar). PDF and Office previews retain extracted-text and metadata fallbacks; EPS reports an actionable unavailable state rather than executing PostScript without a vetted isolated renderer.
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
| Ctrl+C / Ctrl+X | Copy / Cut (same-filesystem Linux Move is supported with safe conflict dispositions; cross-filesystem and Windows Move reject without mutation) |
| Ctrl+V | Paste |
| Delete | Move to trash |
| Shift+Delete | Permanently delete |
| F2 | Rename |
| Any printable key | Open command frame |

---

## Build from Source

Requires Rust 1.94.0. CI and release workflows pin this exact toolchain for Linux and Windows builds. macOS packaging is deferred.

```sh
git clone https://github.com/hoozter/ottrin.git
cd ottrin
cargo run -p ottrin-app
```

Build release packages into `dist/`:

```sh
./build-all.sh
```

Linux dependencies:

```sh
sudo apt-get install -y \
    libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
    libxkbcommon-dev libssl-dev
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

v0.1.0 is in release hardening. Linux AppImage/portable artifacts and Windows portable/installer artifacts are the current release path. See [docs/USER_GUIDE.md](docs/USER_GUIDE.md), [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md), and [docs/ROADMAP.md](docs/ROADMAP.md).

## License

MIT

## Author

Made by [hoozter](https://hoozter.com) — [github.com/hoozter/ottrin](https://github.com/hoozter/ottrin)
