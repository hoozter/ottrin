# Ottrin — Project Definition

## Vision

Ottrin is the file manager for people who have one hand on the keyboard and one hand on the
mouse. It bridges the gap between terminal power users and GUI browsing without trying to be
both at once. It is fast, beautiful, and immediately familiar — no learning curve for anyone
who has used a browser or macOS Finder, with meaningful depth for users who know their way
around a terminal.

**Target user:** Developers, technical users, and power users who use the terminal regularly
but still want a GUI for visual browsing, previewing, and organizing files. People who find
Nautilus/Files too slow and simple, and find the terminal too limited for visual tasks.

**One-sentence pitch:** The file manager that terminal users don't feel embarrassed using.

---

## Core UX Principles

1. **Where am I?** The user always knows their exact location in the filesystem. The UI never
   leaves them disoriented. Miller view shows the full parent chain at all times.

2. **Up always works.** No matter how the user got to the current folder — bookmark, search
   result, typed path, command — pressing Up or `cd ..` always navigates to the real parent.
   The hierarchy is always computed from the filesystem, never from navigation history.

3. **Keyboard first, mouse never required.** Every action reachable with a keyboard. Arrow
   keys navigate the file list at all times (unless the command frame is focused). The command
   frame supports terminal-like input for users who prefer typing.

4. **Speed above all.** The UI never blocks. Directory listings load in the background.
   Navigation is instant. Search results appear before the user finishes typing (when indexed).
   Animations are either instant or under 150ms. Nothing ever makes the user wait.

5. **No modes, no sidebars, no clutter.** One clean window. Tabs for multiple locations.
   Target bar for copy/move destinations. No sidebar full of pinned links. No global app-mode
   switches. The UI stays out of the way.

---

## Layout Anatomy

```
┌────────────────────────────────────────────────────────────────────────┐
│  [←][→][↑]   /home/campbell/Projects/Ottrin          [⌘K]   [≡]       │  ← Window chrome
├────────────────────────────────────────────────────────────────────────┤
│  📁 Ottrin  ×  │  📁 Downloads  ×  │  /mnt/usb  ×  │        [+]        │  ← Tab bar
├────────────────────────────────────────────────────────────────────────┤
│                                                                        │
│  [home] / [campbell] / [Projects] / Ottrin          [≡ List ▾]        │  ← Breadcrumb + view switcher
│  ──────────────────────────────────────────────────────────────────    │
│  home           │  Projects        │  Ottrin                          │
│  ──────────     │  ──────────      │  ──────────                      │  ← Miller columns (default)
│  campbell  ▶    │  Ottrin      ▶   │  📁 crates                      │
│  shared         │  website         │  📁 docs                        │
│  guest          │  scripts         │  📄 Cargo.toml      12 KB       │
│                 │                  │  📄 README.md        3 KB       │
│                 │                  │                                  │
├────────────────────────────────────────────────────────────────────────┤
│  🎯  ~/Downloads                           [Recent ▾]       [✕]        │  ← Target bar
├────────────────────────────────────────────────────────────────────────┤
│  4 items · Cargo.toml selected (12 KB)                  ● Idle  [>_]   │  ← Status bar
└────────────────────────────────────────────────────────────────────────┘
```

**Command frame** (slides up from bottom when activated, auto-hides when dismissed):
```
├────────────────────────────────────────────────────────────────────────┤
│  > cd ~/Dow█                                                           │
│  ┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄   │
│    ~/Downloads                                                         │
│    ~/Documents                                                         │
└────────────────────────────────────────────────────────────────────────┘
```

---

## Feature Specifications

### Navigation

- **Back / Forward** buttons with per-tab history. Each tab maintains its own stack.
- **Up** button and `cd ..` always navigate to the real filesystem parent. Works regardless
  of how the current folder was reached.
- **Arrow keys** always control the file list:
  - `↑` / `↓` — move selection
  - `→` — enter selected folder
  - `←` — go up to parent
  - `Enter` — open (enter folder, or open file with default app)
  - Arrows are captured by the command frame only when it is focused
- **Address bar** shows current path as clickable breadcrumbs. Click any segment to navigate
  there. Click the path text to edit it directly (for direct path entry).
- **Home** shortcut always navigates to the user's home directory.

### View Modes (per-tab)

- **Miller columns** (default) — shows parent, current, and child/preview columns side by
  side. Rightmost column previews selected item inline (file preview or folder contents on
  hover). Navigating enters a new rightmost column; going up shifts columns right.
- **List** — traditional single-pane file list with columns (Name, Size, Modified, Type).
  Column widths draggable. Click column header to sort.
- **Grid / Thumbnails** — icon grid. Thumbnails generated for images.
- View mode switch is per-tab, accessible from the breadcrumb row or keyboard shortcut.

### Tabs

- Full browser-style tabs with independent path and navigation history per tab.
- `Ctrl+T` — new tab (opens in home directory)
- `Ctrl+W` — close current tab
- `Ctrl+Tab` / `Ctrl+Shift+Tab` — cycle tabs
- Middle-click tab to close
- Hover tab for full path tooltip
- Tab label shows folder name (not full path)
- Tabs restore on next launch (optional, configurable)

### Link View (Split Tabs)

- Right-click any tab → "Link with..." → pick a second tab → window splits 50/50
- Divider is draggable
- Right-click either tab → "Unlink" to return to single pane
- Right-click left tab → "Pin" — left panel frozen; clicking other tabs shows them in the
  right panel only. This creates an on-demand Commander-style layout.
- Keyboard: `Ctrl+\` to toggle link with the last used tab

### Target System

Replaces dual-pane as the primary copy/move destination mechanism.

- **Setting a target:**
  - Right-click any folder → "Set as Target"
  - Drag any folder to the target bar
  - Type a path directly in the target bar
  - Click "Recent ▾" in the target bar → pick from last 10 targets
- **Using the target:**
  - Select any file(s) anywhere in any tab
  - Right-click → "Copy to Target" or "Move to Target" (prominent, at top of context menu
    when a target is set)
  - Multiple files from multiple tabs can be sent to the same target in sequence
- **Visual feedback:** when the target folder is visible in the current view, it receives a
  subtle accent highlight in the file list
- **Target persists** across tab switches and sessions until explicitly cleared
- **Recent targets:** last 10, persisted to config file

### Command Frame

A terminal-inspired command input that slides up from the bottom of the window. Not a full
terminal emulator — a file-management-aware command interface.

**Activation:**
- Press `:` or type any printable character while the file list has focus
- Click the `>_` icon in the bottom-right of the status bar
- Option in settings: "Always show command frame" (pinned open permanently)

**Behavior:**
- Shows current input with a `>` prompt
- Tab completion for paths (shows dropdown of matches)
- `↑` / `↓` cycles through command history
- `Escape` dismisses without action
- `Enter` executes

**Supported commands:**
| Command | Action |
|---|---|
| `cd /path` or `/path` or `~/path` | Navigate to path |
| `cd ..` | Go up one level |
| `mkdir name` | Create folder, navigate into it |
| `touch file.txt` | Create file |
| `cp file /dest` | Copy file (dest tab-completes) |
| `mv file /dest` | Move file (dest tab-completes) |
| `mv file` | Move to current Target (if set) |
| `cp file` | Copy to current Target (if set) |
| `rm file` | Move to trash (NOT permanent delete) |
| `rm -f file` | Permanent delete (asks confirmation) |
| `chmod +x file` | Make executable |
| `chmod 755 file` | Set permissions |
| `chown user:group file` | Change ownership |
| `ln -s /target /link` | Create symlink |
| `terminal` or `cmd` | Open user's preferred terminal at current path |

Every command immediately reflects in the GUI file list on completion.

### Spacebar Preview

- `Space` — open full preview overlay. `Space` or `Escape` closes it.
- `←` / `→` — cycle through files in current folder while preview is open
- In Miller view, rightmost column shows an inline compact preview when a file is selected

**Preview support by type:**
| Type | Preview |
|---|---|
| Text, code, markdown | Syntax-highlighted content, scrollable |
| Images (jpg, png, gif, webp, svg, bmp) | Full rendered image, scroll to zoom |
| Video | First frame thumbnail + duration/resolution metadata |
| Audio | Metadata (artist, album, duration, bitrate) + waveform if feasible |
| PDF | Rendered first page + page count |
| Archives (zip, tar, gz, bz2, xz, 7z) | File listing with names and sizes |
| Office docs | Extracted text / metadata |
| Unknown | Hex dump (first 512 bytes) + file type guess |

### File Operations

Standard operations, all async (never block the UI thread):

- Create file / folder
- Rename (F2 or click-to-rename)
- Copy (Ctrl+C), Cut (Ctrl+X), Paste (Ctrl+V)
- Copy to Target / Move to Target (via context menu or keyboard)
- Delete to trash (Delete)
- Permanent delete (Shift+Delete, with confirmation)
- Open with default app (Enter or double-click)
- Open with... (right-click menu)
- Properties panel (file size, type, modified/created dates, path, permissions)
- Drag and drop between tabs and to/from target bar

### Copy Engine

- Queue-based: all copy/move operations go through a transfer queue
- Per-job progress: bytes transferred, total, speed, ETA
- Conflict resolution: Skip / Overwrite / Rename — global default + per-item override
- Cancel and retry support
- Visible in a collapsible panel (accessible from status bar)

### Search (Phase 2)

Indexed, instant, global. Built into the app, not relying on external tools.

- `Ctrl+F` or click the search icon → search overlay appears
- Results appear instantly (indexed) or within milliseconds for current folder
- **Scope toggles:** Current Folder | Home Directory | All Indexed
- Results show: filename, full path, size, modified date
- Keyboard navigable: arrow keys, Enter to open, `Ctrl+Enter` for containing folder
- **Index configuration:** settings panel to add/remove indexed paths, exclude patterns
  (e.g., `node_modules`, `.git`, build artifacts), toggle hidden files, toggle system files
- Index builds in background on first launch; updates incrementally via inotify (Linux) /
  FSEvents (macOS) / ReadDirectoryChangesW (Windows)
- Tech: SQLite FTS5 for v1, tantivy upgrade path

### Keyboard Shortcuts Reference

| Key | Action |
|---|---|
| `↑` / `↓` | Move selection |
| `←` | Go up to parent folder |
| `→` or `Enter` | Enter folder / open file |
| `Backspace` | Go up to parent (alternative) |
| `Space` | Open/close preview |
| `Escape` | Close preview / dismiss command frame / cancel rename |
| `F2` | Rename selected |
| `Delete` | Move to trash |
| `Shift+Delete` | Permanent delete (with confirmation) |
| `Ctrl+C` | Copy |
| `Ctrl+X` | Cut |
| `Ctrl+V` | Paste |
| `Ctrl+A` | Select all |
| `Ctrl+T` | New tab |
| `Ctrl+W` | Close tab |
| `Ctrl+Tab` | Next tab |
| `Ctrl+Shift+Tab` | Previous tab |
| `Ctrl+\` | Toggle link view with last tab |
| `Ctrl+L` | Focus address bar |
| `:` or any char | Open command frame |
| `Ctrl+F` | Open search |
| `Ctrl+R` or `F5` | Refresh |
| `Alt+←` | Navigate back |
| `Alt+→` | Navigate forward |
| `Alt+↑` | Navigate up |

### Themes and Appearance

- Dark mode and Light mode. System default auto-detection.
- True dark (`#141414` background, `#1e1e1e` panels, 85% white text).
- True light (white background, warm neutral panels).
- Both modes designed together — consistent visual language, not two separate themes.
- File type icons: Papirus icon set (open source, consistent, high quality).
- System fonts: Segoe UI on Windows, system-ui/Inter on Linux.
- No custom fonts to load — fast startup, consistent with OS.

---

## Architecture Principles

1. **Never block the UI thread.** All filesystem I/O, directory listings, file operations,
   preview loading, and index queries run on background threads. Results are sent to the UI
   via channels.
2. **Keep domain logic independent from egui.** The `ottrin-core` crate has no UI dependencies.
3. **Platform behavior is isolated.** `ottrin-platform` abstracts OS differences behind traits.
4. **Fail gracefully.** Every operation has a clear fallback. No silent failures.
5. **Measure performance from day one.** Startup time, frame time, and listing latency are
   tracked and visible during development.

---

## Non-Goals

- Full terminal emulator (use "Open Terminal Here" instead)
- Plugin/extension system (v1)
- Cloud storage backends (v1)
- Per-folder layout overrides (v1)
- macOS support (v1 — Linux and Windows only; macOS is a future target)
- Mobile/touch-first interface

---

## Tech Stack

- **Language:** Rust (edition 2024)
- **UI:** egui / eframe (immediate-mode, native rendering)
- **Icons:** Papirus (bundled subset)
- **Config:** JSON via serde, stored in platform config dir
- **Search index:** SQLite FTS5 (phase 2)
- **File watching:** notify crate (cross-platform inotify/FSEvents/ReadDirectoryChangesW)
- **Image decoding:** image crate
- **Audio metadata:** symphonia
- **Trash:** trash crate
