# Ottrin — Project Definition (Current)

## Vision
Ottrin is a fast, keyboard-first desktop file manager with a polished GUI workflow. It keeps Miller-column navigation at the core while adding practical power-user features: command frame, semantic file styling, integrated search, drop-folder operations, and privilege-aware retries.

## Current product model
- Core platform: Rust + egui/eframe 0.33 desktop app.
- Primary UX: column (Miller) navigation with smooth horizontal auto-scroll and persistent column sizing.
- Secondary views: list and grid.
- Smart panel: `Info`, `Drop Folder`, and `Search` sections.
- Command frame: in-app command surface for common file operations and path navigation.
- Settings modal: separate egui viewport; tabs for General, Appearance, Files, Search, Cache.

## UX principles
1. Path clarity first: users should always understand where they are.
2. Focus must be predictable: keyboard navigation should never "lose" selection context.
3. Operations stay in-app: move/copy/search/theme editing should not depend on external UIs.
4. Responsiveness over decoration: keep interactions immediate and stable.
5. Progressive depth: simple defaults, advanced controls when needed.
6. User control over engine: no config is hardcoded in the search engine — all exclusions and rules are user-visible and removable.

## Implemented capabilities

### Navigation and layout
- Finder-style Miller columns with depth tracking, scroll freeze, and remembered path re-entry.
- Horizontal auto-scroll keeping focused column visible; viewport frozen on Up/Down.
- Resizable per-folder columns with persisted widths.
- Tabs with per-tab history and independent location state.
- Bookmarks row with add/remove support.

### File operations
- Copy, move, rename, trash, permanent delete.
- Conflict strategy support (default rename behavior).
- Context menu actions for destination workflows.

### Drop Folder workflow
- Set/clear/change current drop folder.
- Pinned and recent drop folders.
- Context actions: copy/move to drop folder.
- One-time empty-folder onboarding hint flow.

### Search
- Dedicated indexed search service (`ottrin-search`) with inotify-driven watcher.
- Global and current-folder scope modes.
- Search panel integrated in smart panel with result context actions (open/reveal/copy/move).
- Live status card in settings: file count, active root during scan, last-scan timestamp.
- User-controlled exclude lists: folders, glob patterns (exclude and include filters).
- Platform-default exclude roots (Linux: /proc /sys /dev /run) pre-populated but removable.
- Supplemental results via system plocate and private user-space locate.db.
- Advanced: daily root updatedb cron job installer, fanotify privileged daemon option.
- Current engine is in-memory; planned migration to SQLite + FTS5 for resumable, scalable indexing.

### Privileged operations
- Helper-backed privilege retry architecture.
- UI status and retry affordances for denied operations.

### Theme system
- Presets: Ottrin, Breeze, Adwaita, Windows 11, Solarized, Nord, G33k.
- In-app theme editor with live preview and save/save-as/reset flows.
- Semantic file styling pipeline shared across views.

### File preview
- Miller column info panel: type descriptions, collapsible metadata, size cycling.
- Text file content preview.
- Image preview with max-height constraint.
- PDF: page count and metadata.
- Audio: tags and duration.
- Archives: zip content listing.

## Semantic file styling architecture
- Classification in `ottrin-core` (`FileSemantic`, `FileCategory`, `CodeSubtype`, `FolderKind`).
- Central style mapping in `ottrin-ui` (icon + color derived from semantic class + state + theme).
- Icon backend: MaterialSymbolsFilled TTF (PUA range, bundled).
- Rendering reused across Miller/List/Grid/Search/Info surfaces.

## Current focus areas
- Search engine foundation: SQLite + FTS5, resumable scanning, and durable index storage.
- Tandem View (dual-pane): side-by-side tabs with pinning and independent navigation.
- Remaining search polish: Ctrl+F shortcut, large-tree benchmarking.
- Theme editor UX cleanup: click-to-edit regions, import/export.
- Space Preview → Quick Inspector: separate OS window for images, PDFs, folders, media.

## Notes
This document reflects the current implementation baseline and near-term direction. Historical specs describing a separate "Target bar" model or old icon/theme assumptions are superseded by this version.
