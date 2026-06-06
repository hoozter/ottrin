# Ottrin Roadmap

Last verified: 2026-06-06 on Linux (`x86_64-unknown-linux-gnu`)

## Legend
- [ ] Not started
- [~] In progress
- [x] Done
- [?] Implemented or documented, but still needs manual/cross-platform verification

## Current Baseline
- [x] `cargo check`
- [x] `cargo test`
- [x] Release-mode 500k search benchmark: 30ms locally without competing builds
- [x] `cargo build --release`
- [x] Linux release binary builds locally
- [x] Strict Clippy gate
- [x] Local Windows GNU target check
- [x] Local Windows GNU release build
- [?] Windows CI workflow exists, but hosted run status is not verified in this local session
- [ ] Windows runtime smoke test on an actual Windows machine
- [ ] Linux package artifact smoke-tested
- [ ] Windows package artifact smoke-tested

## v0.1.0 — Foundation

### Core Navigation and Views
- [x] Keyboard-first navigation
- [x] Miller columns with stable focus and path-aligned selection
- [x] List and Grid views
- [x] Tabs with per-tab history
- [x] Bookmarks row and path/address bar
- [x] Resizable Miller columns

### File Operations
- [x] Copy, move, rename, delete through platform command model
- [x] Trash and permanent delete modes
- [x] Integrated command frame for shell-like file commands
- [x] Drop Folder workflow with recents and pinned model
- [?] Context-menu Rename/Delete/Copy/Cut actions require manual QA

### Privileged Operations
- [x] Helper architecture and request/response model
- [x] Linux `pkexec` execution path in code
- [x] Windows PowerShell/UAC launcher path in code
- [ ] Linux polkit packaging smoke test
- [ ] Windows UAC/helper smoke test

### Preview and Theming
- [x] Text and image preview paths
- [x] Metadata-oriented preview support for several file categories
- [x] Theme preset engine and custom overrides
- [x] Semantic file classification and centralized visual mapping

## v0.2 — Search Overhaul and Tandem View

### Search Reliability
- [x] Indexing progress moved from misleading percent to live file count
- [x] Watcher-triggered re-index on filesystem changes
- [x] Recursive walks through configured roots
- [x] `Field::Any` matches name and full path
- [x] Manual `rebuild_index()` method
- [x] Current-folder fallback walk has unit coverage
- [x] 500k-item query benchmark has unit coverage
- [x] Release-mode benchmark passed locally at 30ms without competing builds
- [?] Current-folder live walk needs large real-tree QA
- [?] Search completeness needs manual QA on Linux and Windows home directories

### Search Settings UI
- [x] Live status card with counts, active root/path, and timestamps
- [x] Editable include roots, excluded folders, include globs, and exclude globs
- [x] Default exclude patterns and platform-default excluded roots
- [x] Supplemental `plocate` and private `locate.db` integration
- [x] Advanced updatedb/fanotify controls
- [x] Reset search settings to defaults
- [x] Search settings gear from search panel
- [x] Non-floating settings scrollbar
- [x] Ctrl+F focuses search panel from normal app flow

### SQLite Search Foundation
- [x] SQLite schema and migration module
- [x] SQLite metadata index store
- [x] Batch writes during indexing
- [x] Load from SQLite on startup
- [x] Resumable scan checkpoints
- [x] Keep previous in-memory index active while rebuilding
- [x] Incremental DB updates for watcher events
- [x] Optional content indexing scope settings/storage
- [x] Opt-in ripgrep-backed content search with snippets
- [ ] True FTS5 content index
- [ ] Background extraction queue and throttling

### Tandem View
- [x] Side-by-side tab display with draggable divider
- [x] "Open in Tandem" via folder context action
- [x] Shift-click two tabs to enter tandem
- [x] Tab pinning
- [x] Independent navigation, scroll, and sort per side
- [x] Active-side keyboard routing
- [x] Copy/move to opposite pane
- [?] Full manual QA across Miller/List/Grid and tab switching

## v0.3 — Theme Editor and Quick Inspector

### Theme Editor Redesign
- [x] Separate theme editor viewport
- [x] Preview-first layout
- [x] Hover/click preview regions
- [x] Region-specific color editors
- [x] Theme import/export as JSON
- [x] Import support for wrapped `SavedTheme`, bare `SavedTheme`, and bare customization payloads

### Quick Inspector
- [ ] Replace Space overlay with separate OS-level window
- [ ] Toggle/close with Space or Escape
- [ ] Window title follows selected filename
- [ ] Image inspector: zoom, pan, checkerboard transparency, EXIF, rotate
- [ ] PDF inspector: rendered pages, navigation, thumbnail strip
- [ ] Folder inspector: item counts, total size, type breakdown, sample thumbnails
- [ ] Media inspector: duration, codec, resolution, bitrate metadata
- [ ] Font inspector: preview text and multiple sizes
- [ ] Archive inspector: content listing without extraction

## v0.4 — Usability Polish

### Settings and Cache
- [ ] Show cache location
- [ ] Clear cache with confirmation and freed-space display
- [ ] Maximum cache size setting
- [ ] Remove confusing/debug settings from user-facing views
- [ ] Consistent settings layout pass

### Multi-Select and Actions
- [ ] Ctrl+A multi-select
- [ ] Shift-click range select
- [ ] Rubber-band selection where appropriate
- [ ] Multi-file context actions
- [ ] Context-menu action/status-message polish

### Tabs and Views
- [ ] Tab drag-and-drop
- [ ] Per-folder view presets
- [ ] Column/profile persistence

### Thumbnails
- [ ] Thumbnail-as-icon toggle for Miller/List/Grid
- [ ] Folder content preview thumbnails
- [ ] Thumbnail cache invalidation

## v0.5 — Release Hardening
- [ ] GitHub Actions or equivalent CI for Linux and Windows
- [ ] Linux AppImage built and smoke-tested
- [ ] Linux deb/rpm decision and packaging pass
- [ ] Windows portable build created and smoke-tested
- [ ] Windows installer candidate built and uninstall path verified
- [ ] Startup/shutdown/crash-path stability pass
- [ ] Manual regression checklist for navigation/search/file ops/tandem/previews
- [x] Clippy cleanup pass for UI style lints
- [ ] User quickstart and known-limitations docs

## Definition of "Daily-Driver Ready"
- [ ] Navigation is stable and predictable in all view modes
- [ ] Search is fast, complete, and trustworthy for normal home/project trees
- [ ] Tandem View is reliable enough for file-management workflows
- [ ] File operations have clear success/failure/retry UX
- [ ] Privileged operations are reliable on Linux and validated on Windows
- [ ] Normal workflows do not block the UI for large folders or long scans
- [ ] Linux and Windows users can install/run without Cargo
