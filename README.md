# Ottrin

**Navigate with clarity.**

A fast, keyboard-first file manager built in Rust with [egui](https://github.com/emilk/egui). Miller columns, blazing-fast indexed search, rich file previews, and a full theme system — no Electron, no web views.

[![CI](https://github.com/hoozter/ottrin/actions/workflows/ci.yml/badge.svg)](https://github.com/hoozter/ottrin/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/hoozter/ottrin)](https://github.com/hoozter/ottrin/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.94.0-orange.svg)](https://www.rust-lang.org)

---

## Download

Pre-built binaries for the [latest release](https://github.com/hoozter/ottrin/releases/latest):

| Platform | File |
|----------|------|
| Linux (x86\_64) | `Ottrin-x.y.z-x86_64.AppImage` |
| Linux portable | `Ottrin-x.y.z-linux-x86_64-portable.tar.gz` |
| Windows portable (x64) | `Ottrin-x.y.z-windows-x64-portable.zip` |
| Windows installer (x64) | `Ottrin-x.y.z-windows-x64-setup.exe` |

Windows MSI and macOS packages are planned; the current v1 hardening path is Linux AppImage/portable plus Windows portable and per-user installer artifacts.

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

Windows uses WGPU when Automatic finds no compatibility signal. For detected
remote sessions and recognized virtual/problem adapters, Automatic selects the
checksum-pinned Mesa 26.1.3 OpenGL compatibility runtime, explains that choice
once, and offers Automatic, Native, and OpenGL compatibility for the next
launch in Settings. `ottrin.exe --renderer opengl` (or
`OTTRIN_RENDERER=opengl`) remains a diagnostic override; its Mesa, pal1000,
and Microsoft notices are bundled under `third-party-notices/`.

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
- **Preview system** — text and images in-app, bounded first-page PDF visuals, bounded DOCX/XLSX/XLSM content-preview visuals, PPTX slide text, and rich archive/media metadata (Space bar). PDF and Office previews retain extracted-text and metadata fallbacks.

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
| Ctrl+C / Ctrl+X | Copy / Cut (Move is disabled in the current hardening build) |
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
