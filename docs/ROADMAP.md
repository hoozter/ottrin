# Ottrin Roadmap (Current)

## Legend
- [ ] Not started
- [~] In progress
- [x] Done

## v0.1.0 — Foundation release (2026-03-26)

**Changelog:** Initial release with stable core. Build issues fixed (PathBuf serialization, closure borrows). All tests pass and release binary builds.

### Core navigation and views
- [x] Keyboard-first navigation (arrows, Enter, Backspace, Tab, Space)
- [x] Column view with stable focus and path-aligned selection behavior
- [x] Finder-style Miller navigation (depth tracking, scroll freeze, remembered path re-entry)
- [x] List and Grid views
- [x] Horizontal column auto-scroll (focus stays visible)
- [x] Resizable column separators
- [x] Tabs with per-tab history
- [x] Bookmarks row and path/address bar

### File operations and command workflow
- [x] Copy / move / delete (trash + permanent)
- [x] Rename
- [x] Integrated command frame with shell-style commands
- [x] Drop Folder workflow with in-app folder picker
- [x] Drop Folder recents and pinned model (core support)

### Privileged operations
- [x] Privileged helper architecture
- [x] Retry-as-admin flow for denied operations
- [x] In-app helper status and setup surface

### Search foundation
- [x] Dedicated search crate (`ottrin-search`)
- [x] Indexed global/current-folder search modes
- [x] Search query parser and sort modes
- [x] Search panel integrated into smart sidebar
- [x] Search result context actions (open/reveal/copy/move workflows)

### Theming and visual system
- [x] Theme preset engine (Ottrin/Breeze/Adwaita/Windows 11/Solarized/Nord/G33k)
- [x] Per-preset custom overrides persisted in config
- [x] Semantic file classification + centralized visual mapping
- [x] In-app theme editor with live preview + save/save-as/reset flow

### Miller file preview
- [x] Collapsible info panel with file type descriptions
- [x] Expandable details (permissions, owner, dimensions, etc.)
- [x] Clickable size cycling (KB/MB/GB/TB)
- [x] Scroll capped — no infinite horizontal scroll
- [x] Image preview with max-height constraint

---

## v0.2 — Search Overhaul + Tandem View

### Search reliability
- [x] Fixed indexing pipeline (0% stall → live file-count progress)
- [x] File watcher (notify) triggers re-index on create/rename/delete
- [x] Recursive walk of all indexed locations (WalkDir)
- [x] Field::Any query matches full path, not just filename
- [x] `rebuild_index()` method for manual re-scan
- [ ] Benchmark: <100ms query on indexes of 500k+ files
- [ ] "Current folder" scope uses live walk fallback — needs testing at scale

### Search settings UI
- [x] Live status card: file count, scan progress, active root, last-scan timestamp
- [x] Framed editable list for indexed locations with per-row remove
- [x] Framed editable list for excluded folders with per-row remove
- [x] Exclude/include glob lists with add-pattern input and per-item remove
- [x] Default exclude patterns in SearchConfig (editor scratch, VCS, Python cache, Trash)
- [x] Platform-default exclude roots (Linux: /proc /sys /dev /run) — user-removable
- [x] Supplemental results: plocate and private locate.db integration with availability detection
- [x] Advanced section: daily updatedb cron (pkexec) + fanotify privileged-daemon option
- [x] "Reset search settings to defaults" — config reset only, does not clear the index
- [x] Gear icon in search panel opens Search settings tab
- [x] Settings viewport scrollbar: non-floating, 8px, always visible
- [ ] Ctrl+F shortcut to focus search panel from anywhere

### Search engine foundation (DB-backed)
- [ ] SQLite index store for file metadata + crawl state
- [ ] Resumable scanner with checkpoints (no restart-from-zero)
- [ ] Keep previous index active while rebuilding
- [ ] FTS5 content index (opt-in by folder)
- [ ] Background extraction queue + throttling
- [ ] Migration path from in-memory index to DB

### Tandem View (dual-pane)
- [ ] Side-by-side tab display with draggable divider
- [ ] "Open in Tandem" via right-click folder
- [ ] Shift+click two tabs to enter tandem
- [ ] Tab pinning (one side stays fixed while other rotates)
- [ ] Independent navigation/scroll/sort per side

---

## v0.3 — Theme Editor + Space Inspector

### Theme editor redesign
- [ ] Click-on-preview to edit: hover highlights regions, click opens editors
- [ ] Single live preview (remove action preview)
- [ ] Fix window clipping / sizing constraints
- [ ] Theme import/export (.json)

### Space Preview → Quick Inspector
- [ ] Separate OS window, not constrained by main app
- [ ] Image: pixel-accurate zoom, pan, checkerboard, EXIF, rotate
- [ ] PDF: rendered pages, navigation, thumbnail strip
- [ ] Folders: item count, size, type breakdown, sample thumbnails
- [ ] Media: metadata display (playback deferred)
- [ ] Fonts: preview text entry
- [ ] Archives: content listing

---

## v0.4 — Polish + Thumbnails

### Settings and cache
- [ ] Cache settings: location, clear, max size, explanation
- [ ] General settings consistency pass

### Thumbnail icons and folder previews
- [ ] Thumbnail-as-icon toggle (Grid/Miller/List)
- [ ] macOS-style folder content preview (composited thumbnails)
- [ ] Lazy generation + cache with mtime invalidation
- [ ] Scope: home folder by default, configurable

### Usability
- [ ] Per-folder view presets and column/profile persistence
- [ ] Context menu and status messaging polish
- [ ] Wire Rename/Delete/Copy/Cut context menu actions (stubs currently)
- [ ] Multi-select (Ctrl+A, Shift+click, rubber band)

---

## v0.5 Release hardening
- [ ] Linux packaging pass (AppImage, deb, rpm)
- [ ] Windows packaging + privilege/elevation QA
- [ ] Startup/shutdown/crash-path stability hardening
- [ ] Regression test coverage for navigation/search/file ops

---

## Definition of "v1 workable"
- [ ] Navigation is stable and predictable in all view modes
- [ ] Search is fast, complete, and trustworthy for common workloads
- [ ] Tandem View works reliably for dual-pane workflows
- [ ] Core file operations are robust with clear failure/retry UX
- [ ] Privileged operations are reliable on Linux and validated on Windows
- [ ] No blocking UI freezes in normal workflows
