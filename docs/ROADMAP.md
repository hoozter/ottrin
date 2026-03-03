# Ottrin — Roadmap

## Legend
- [ ] Not started
- [~] In progress
- [x] Done

---

## Phase 0 — Foundation ✓
- [x] Rust workspace scaffold with split crates
- [x] CI baseline (cargo check on Linux + Windows)
- [x] Core crate: domain models, AppState, config serialization
- [x] Platform crate: trash, file ops abstraction
- [x] Copy crate: transfer queue model
- [x] Preview crate: async pipeline
- [x] Project definition and roadmap documented

---

## Phase 1 — UI Shell (Current Phase)

Goal: A navigable, beautiful window with the correct chrome. No feature completeness yet —
just the skeleton that looks and feels right.

### Window Chrome
- [ ] Title bar: back, forward, up buttons
- [ ] Address bar: shows current path as clickable breadcrumbs
- [ ] Address bar: click to edit path directly (Ctrl+L)
- [ ] Search button (opens overlay — stub for now)
- [ ] Menu button (hamburger — stub for now)

### Tab Bar
- [ ] Tab strip rendering with active/inactive states
- [ ] New tab button (+)
- [ ] Close tab (× on tab, middle-click)
- [ ] Keyboard: Ctrl+T, Ctrl+W, Ctrl+Tab, Ctrl+Shift+Tab
- [ ] Each tab has independent path + navigation history
- [ ] Tab label = folder name; tooltip = full path

### File Pane — List View
- [ ] Directory listing with async background worker (port existing)
- [ ] Columns: Name, Size, Modified, Type
- [ ] Sortable columns (click header)
- [ ] Keyboard navigation: ↑↓ move selection, → enter folder, ← go up, Enter open
- [ ] Double-click to enter folder / open file
- [ ] Selection: click, Ctrl+click (multi), Shift+click (range), Ctrl+A
- [ ] Smooth hover states, selection highlight
- [ ] Empty folder message

### File Pane — Miller Columns (Default)
- [ ] Three-column layout: grandparent | parent | current
- [ ] Column scroll: entering a folder adds rightmost column, old leftmost drops off
- [ ] Going up shifts columns right
- [ ] Rightmost column shows folder contents on hover, file preview on select
- [ ] Arrow key navigation across columns (← enters parent column, → enters child)
- [ ] Consistent with "Up always works" rule

### File Pane — Grid / Thumbnails
- [ ] Grid layout with configurable icon size
- [ ] Thumbnail generation for image files
- [ ] Fallback icon for non-image files (Papirus set)

### View Mode Switcher
- [ ] Per-tab view mode: Miller | List | Grid
- [ ] Accessible from breadcrumb row button
- [ ] Persisted per session

### Target Bar
- [ ] Always-visible bar between file pane and status bar
- [ ] Shows "No target set" when empty (dim, unobtrusive)
- [ ] Shows target path with accent color when set
- [ ] "Recent ▾" dropdown — last 10 targets, persisted
- [ ] Clear (✕) button
- [ ] Right-click any folder in file list → "Set as Target"
- [ ] Drag folder to target bar to set it
- [ ] Type path directly in target bar

### Status Bar
- [ ] Item count for current folder
- [ ] Selection summary (N selected, X MB)
- [ ] Operation status indicator (Idle / copying... / error)
- [ ] `>_` icon to open/close command frame

### Command Frame
- [ ] Slide-up panel from bottom, above status bar
- [ ] Activated by: typing `:`, clicking `>_` icon
- [ ] Option: always-visible (settings)
- [ ] Input with `>` prompt
- [ ] Path tab-completion with dropdown
- [ ] Command history (↑/↓)
- [ ] Escape dismisses
- [ ] Supported commands: cd, mkdir, touch, cp, mv, rm, chmod, chown, ln, terminal/cmd

### Dark / Light Theme
- [ ] True dark theme (#141414 bg, #1e1e1e panels)
- [ ] True light theme (white bg, warm neutral panels)
- [ ] System default detection (auto-select on launch)
- [ ] Manual toggle in menu

---

## Phase 2 — File Operations

Goal: All standard file operations work correctly and asynchronously.

- [ ] Create file / folder (from context menu and command frame)
- [ ] Rename: F2 inline edit
- [ ] Copy / Cut / Paste (Ctrl+C/X/V)
- [ ] Copy to Target (context menu, prominent)
- [ ] Move to Target (context menu, prominent)
- [ ] Delete to trash (Delete key + context menu)
- [ ] Permanent delete (Shift+Delete, confirmation dialog)
- [ ] Open with default app
- [ ] Open with... (lists installed apps)
- [ ] Properties panel (size, type, dates, path, permissions)
- [ ] All operations async — file ops use background thread, results reflected in UI
- [ ] Transfer queue with progress, speed, ETA
- [ ] Conflict resolution: Skip / Overwrite / Rename (global default + per-item)
- [ ] Cancel and retry in transfer queue
- [ ] Transfer queue panel (collapsible from status bar)
- [ ] Drag and drop between tabs

---

## Phase 3 — Spacebar Preview

Goal: Space previews anything. Inline preview in Miller rightmost column.

- [ ] Preview overlay: Space opens, Space/Escape closes
- [ ] ← / → cycle files in current folder while preview open
- [ ] Text / code: syntax highlighted, scrollable
- [ ] Images: full rendered (jpg, png, gif, webp, svg, bmp)
- [ ] Video: first-frame thumbnail + metadata
- [ ] Audio: metadata + waveform (symphonia)
- [ ] PDF: rendered first page + page count
- [ ] Archives: file listing
- [ ] Office: extracted text / metadata
- [ ] Unknown: hex dump + file type guess
- [ ] Preview cache
- [ ] Async loading (no UI block)
- [ ] Miller inline preview in rightmost column

---

## Phase 4 — Link View (Split Tabs)

Goal: On-demand Commander-style layout via tab linking.

- [ ] Link two tabs: right-click tab → "Link with..." → pick second tab
- [ ] Split view: 50/50 with draggable divider
- [ ] Unlink: right-click → "Unlink"
- [ ] Pin: left panel frozen, other tabs show in right panel
- [ ] Keyboard toggle: Ctrl+\
- [ ] Visual indicator on linked tabs

---

## Phase 5 — Search (Indexed)

Goal: Instant global search. The "Everything for Linux/Windows" feature.

- [ ] Search overlay: Ctrl+F
- [ ] Scope: Current Folder | Home | All Indexed
- [ ] Instant results from index
- [ ] Results: filename, full path, size, modified date
- [ ] Keyboard navigation in results
- [ ] Enter → open file; Ctrl+Enter → open containing folder; Ctrl+C → copy path
- [ ] SQLite FTS5 index
- [ ] Background index builder (runs on first launch)
- [ ] Incremental updates via notify crate (inotify / FSEvents / RDCW)
- [ ] Settings: add/remove indexed paths
- [ ] Settings: exclude patterns (node_modules, .git, etc.)
- [ ] Settings: toggle hidden files, system files
- [ ] Index progress indicator on first build

---

## Phase 6 — Polish and Release

- [ ] File system watcher: auto-refresh open folders on external changes
- [ ] Session restore: reopen last session's tabs on launch
- [ ] Bookmarks (per-tab or global — to be decided)
- [ ] Keyboard shortcut customization
- [ ] Settings panel (full)
- [ ] Linux packaging: AppImage, .deb, .rpm
- [ ] Windows packaging: MSI + portable
- [ ] Crash reporting strategy
- [ ] Versioned config migration
- [ ] README with screenshots

---

## Backlog (Future Consideration)
- [ ] macOS support
- [ ] Network paths / mounts UX
- [ ] Per-folder view overrides
- [ ] Theme customization (accent color picker)
- [ ] Bulk rename tool
- [ ] Archive creation / extraction in-app
- [ ] Git status indicators in file list
- [ ] FTP / SFTP remote browsing
