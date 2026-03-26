# EXECUTION PLAN

## Phase 1 — Search Overhaul ("Everything"-class search)
**Priority: Highest — core broken functionality**

### 1A. Search engine reliability
- [x] Diagnose why index stays at 0% — fixed: replaced misleading % with file count
- [x] Ensure file watcher (notify) triggers re-index on file create/rename/delete — verified working
- [x] Verify all indexed locations are walked recursively — WalkDir recursive walk confirmed
- [ ] Benchmark: index ~500k files, query should return in <100ms
- [ ] "Current Folder" scope must do a live walk, not depend on stale index — has fallback, needs testing
- [x] Ensure search finds folders, subfolders, AND files — fixed: Field::Any now matches path too
- [x] Added `rebuild_index()` method for manual reindex trigger

### 1B. Search settings UX overhaul
- [x] Indexed Locations: framed list showing saved paths with Remove button per row; Add folder button below
- [x] Excluded Paths: framed list with Remove per row; Add folder button
- [x] Include/Exclude Globs: framed list with Remove per item; Add pattern input
- [x] Placeholder text clearly styled as placeholder (italic, muted)
- [x] "Rebuild index" button — full-width, clearly styled, disabled while indexing
- [x] "Reset search settings to defaults" at page bottom — resets config only, does not clear index
- [x] Gear icon in search sidebar panel → opens Search settings tab
- [x] Live status card: file count, scan progress, active root, last-scan time, status-tinted border
- [x] Default exclude patterns in SearchConfig (editor scratch, VCS, Python cache, Trash)
- [x] Platform-default exclude roots (Linux: /proc /sys /dev /run) — user-removable
- [x] Supplemental results: plocate + private locate.db integration with availability detection
- [x] Advanced section (collapsible): updatedb cron installer + fanotify daemon option
- [x] Settings viewport scrollbar: non-floating, 8px wide, visible track

### 1C. Search panel UX
- [ ] Ctrl+F focuses search panel from anywhere
- [x] Clear progress reporting: "Indexing · N files found" / "Indexed N files"
- [x] Last indexed timestamp visible in settings status card
- [x] Result count in header (already was there)

### 1D. DB-backed search foundation (SQLite + FTS5)
- [x] Define SQLite schema (files, roots, scan_state, config, optional content index)
- [x] Add DB module in `ottrin-search` with open/init + migration versioning
- [x] Persist index to SQLite in batches (no behavior switch yet)
- [x] Load from SQLite on startup (replace JSON cache)
- [x] Resumable scan checkpoints (resume after restart)
- [x] Keep previous index active while rebuild runs
- [x] Resumable scan checkpoints (resume after restart)
- [x] Keep previous index active while rebuild runs
- [x] Optional content indexing scope (settings + storage)

---

## Phase 2 — Tandem View (dual-pane)
**Priority: High — core Ottrin feature**

### 2A. Data model
- [ ] `TandemState` struct: `left_tab_id`, `right_tab_id`, `pinned_tab_id: Option`, `active_side: Left|Right`
- [ ] Tandem activates via: right-click folder → "Open in Tandem" (creates new tab, shows both)
- [ ] Tandem activates via: Shift+click second tab (shows both selected tabs)
- [ ] Switching to a non-tandem tab → single view (tandem pair preserved in memory)
- [ ] Re-selecting both tandem tabs → restores tandem view

### 2B. Layout
- [ ] CentralPanel splits into left/right halves with draggable divider
- [ ] Each side renders its own Miller/List/Grid view independently
- [ ] Each side has its own address bar, sort controls, scroll state
- [ ] Shared: bookmarks row, drop folder, tab bar (tabs marked with tandem indicator)

### 2C. Pinning
- [ ] Pin button per side in tandem mode
- [ ] Pinned tab stays fixed; switching tabs replaces the unpinned side
- [ ] Only one tab pinned at a time
- [ ] Visual indicator on pinned tab (pin icon)

### 2D. Keyboard
- [ ] Tab/Shift+Tab or Ctrl+Arrow to switch active side
- [ ] All navigation keys work within the active side
- [ ] File operations (copy/move to other side) via drop folder or direct shortcut

---

## Phase 3 — Theme Editor Redesign
**Priority: Medium — UX polish**

### 3A. Interactive preview with click-to-edit
- [ ] Single large live preview (remove the separate "action preview")
- [ ] Hover over preview regions → highlight with overlay + region name tooltip
- [ ] Regions: title bar, nav bar, bookmarks, column bg, column selected row, column text, sidebar, status bar, accent, borders
- [ ] Click region → shows that region's color editors below the preview
- [ ] Global/quick settings (dark/light base, preset selector, save/export) always visible above preview

### 3B. Window fixes
- [ ] Fix theme window clipping on the right side
- [ ] Proper minimum size and resize constraints
- [ ] Theme window should be a separate OS window if possible, or at minimum not clipped by main frame

### 3C. Import/Export
- [ ] Export current theme as .json
- [ ] Import theme from .json file
- [ ] Share-friendly format with all role colors

---

## Phase 4 — Space Preview → Quick Inspector
**Priority: Medium — enhancement**

### 4A. Foundation
- [ ] Replace current space overlay with a separate OS-level window (egui ViewportBuilder)
- [ ] Window shows at reasonable default size, freely resizable, not constrained by main app
- [ ] Close on Escape or Space (toggle)
- [ ] Window title = filename

### 4B. Image inspector
- [ ] Display at actual pixel size initially; show zoom level when zoomed
- [ ] Scroll-to-zoom (Ctrl+scroll or plain scroll, configurable)
- [ ] Pan with click-drag when zoomed
- [ ] Checkerboard background for transparency
- [ ] EXIF data toggle panel (camera, lens, ISO, aperture, GPS if present)
- [ ] Color profile info display
- [ ] Quick rotate (90° CW/CCW buttons)
- [ ] Copy to clipboard, Delete file actions

### 4C. PDF inspector
- [ ] Render actual pages (using pdf rendering library — pdfium-render or similar)
- [ ] Page navigation (prev/next, page number input)
- [ ] Thumbnail strip sidebar
- [ ] Page count display
- [ ] Text selection (if feasible with rendering lib)

### 4D. Folder inspector
- [ ] Item count (files/folders breakdown)
- [ ] Total size (async calculation, show spinner while computing)
- [ ] Top file types breakdown (e.g., "42 images, 18 documents, 7 archives")
- [ ] Sample thumbnails (first N images found)
- [ ] Last modified items list

### 4E. Media inspector (v1: metadata only)
- [ ] Duration, resolution, codec, audio channels, bitrate
- [ ] Subtitle track listing if present
- [ ] Placeholder for future playback controls

### 4F. Other types
- [ ] Fonts: preview text entry field, render sample at multiple sizes
- [ ] Archives: content listing without extracting (zip/tar/gz)
- [ ] Unknown: file metadata + "Open with" app suggestions

---

## Phase 5 — Settings & Cache Cleanup
**Priority: Medium — UX polish**

### 5A. Cache settings
- [ ] Show cache location path
- [ ] Option to move cache to different location
- [ ] "Clear Cache" button with confirmation + freed space display
- [ ] "Maximum Cache Size" setting with slider or input
- [ ] Brief explanation: "Ottrin caches thumbnails and preview data to speed up browsing"
- [ ] Remove the confusing "last selected folder" display

### 5B. General settings cleanup
- [ ] Ensure each settings section has a clear purpose
- [ ] Remove any debug/developer info from user-facing settings
- [ ] Consistent control layout across all settings sections

---

## Phase 6 — Thumbnail Icons & Folder Previews
**Priority: Lower — visual enhancement**

### 6A. Thumbnail-as-icon toggle
- [ ] Setting: "Show thumbnails as file icons" (per-view toggle: Miller, List, Grid)
- [ ] Grid view: thumbnail replaces the icon for supported image types
- [ ] Miller/List: smaller thumbnail in icon position
- [ ] Falls back to semantic icon when thumbnail unavailable or loading
- [ ] Toggle lives alongside semantic color toggle in settings

### 6B. Folder content preview (macOS-style)
- [ ] Composite preview: 2–4 sample thumbnails from folder contents arranged on folder icon
- [ ] Lazy-loaded: generate on first view, cache result
- [ ] Cache invalidation: re-generate when folder mtime changes
- [ ] Scope setting: "Home folder only" (default) or "Global"
- [ ] Config option in Settings > Files

---

## Execution order recommendation
1. **Search overhaul + DB foundation** (1A→1B→1C→1D) — fixing core reliability and persistence
2. **Tandem View** (2A→2B→2C→2D) — missing core feature
3. **Theme editor redesign** (3A→3B→3C) — UX improvement
4. **Space Inspector** (4A→4B→4C→4D→4E→4F) — biggest scope, benefits from stable base
5. **Settings/Cache** (5A→5B) — quick wins, can interleave
6. **Thumbnails** (6A→6B) — polish, depends on stable caching

Rule:
Work through phases in order. Within each phase, complete all sub-items before moving on.
Phases 5 can be interleaved with others as quick-win breaks.
Do not start Phase 4 (Space Inspector) until Phase 2 (Tandem) is stable.

## Completed
- Miller file preview panel (type descriptions, collapsible info, size cycling, scroll cap)
- Finder-style Miller navigation (depth tracking, scroll freeze, remembered paths)
- cd command fixes (tab completion, enter-folder behavior)
- Smart panel layout, breadcrumbs, column width, archive/audio/PDF previews
- Search settings overhaul (Phase 1B + 1C): full settings UI, status card, default excludes, supplemental results, advanced system options, scrollbar fix
