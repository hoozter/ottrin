# Ottrin

**Navigate with clarity.**

A fast, keyboard-friendly file manager built with Rust and [egui](https://github.com/emilk/egui).

---

## Features

- **Column view (Miller columns)** — browse your file system the way it actually works, with parent, current, and preview columns side-by-side
- **Smooth column navigation** — horizontal auto-scroll keeps focus visible while preserving path context
- **Resizable columns** — drag separators; column widths persist while you navigate
- **List and Grid views** — switch between views instantly with a single click
- **Smart sidebar** — info, drop folder, and search panels in a compact, toggleable activity strip
- **Bookmarks bar** — one-click shortcuts to your most-used folders (right-click to add/remove)
- **Integrated command frame** — run `mkdir`, `touch`, `mv`, `cp`, `rm`, `chmod`, `ln -s` and more without leaving the app; opens on any keypress
- **Keyboard first** — arrow keys, Enter, Backspace, and Tab navigate the Miller view
- **Tabs** — multiple locations open at once, full navigation history per tab
- **Theme system** — built-in presets (Ottrin, Breeze, Adwaita, Windows 11, Solarized, Nord, G33k) with live editing and custom theme save/load
- **Semantic file styling** — file/folder types are classified and styled consistently across views
- **Drop Folder workflow** — quick copy/move destination with recents and pinned entries
- **Integrated privilege management** — denied operations can retry via helper flow
- **Global search panel** — indexed search with scope switching and per-result actions
- **Preview system** — text inline, images in-app, and richer metadata cards for PDF/audio/video/archive/office formats (Space bar)

## Status

Pre-release (v0.1.0). Core navigation, file operations, and views are working. Active development.

## Building

Requires Rust 1.85+ (edition 2024).

```sh
git clone https://github.com/hoozter/ottrin.git
cd ottrin
cargo run -p ottrin-app
```

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
| Shift+Delete | Delete permanently |
| F2 | Rename |
| Any printable key | Open command frame |

## Workspace

| Crate | Purpose |
|-------|---------|
| `ottrin-app` | Native entrypoint (eframe) |
| `ottrin-ui` | egui application shell and all views |
| `ottrin-core` | Domain models, commands, settings |
| `ottrin-platform` | OS integrations (trash, shell actions, privileged helper flow) |
| `ottrin-copy` | Copy/move queue with conflict policies |
| `ottrin-search` | Indexed search, watcher sync, and fallback querying |
| `ottrin-preview` | Preview request/response pipeline |

## License

MIT

## Author

Made by [hoozter](https://hoozter.com) — [github.com/hoozter/ottrin](https://github.com/hoozter/ottrin)
