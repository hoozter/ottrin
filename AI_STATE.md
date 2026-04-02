PROJECT
Name:
Ottrin

Purpose:
Keyboard-first graphical file manager with Finder-style Miller columns, integrated file operations, search, previews, theming, and privileged-operation retry flows.

Main technologies:
Rust
egui / eframe 0.33
serde
notify
walkdir

CURRENT STATUS
Workspace builds cleanly with `cargo build` (only deprecation warnings: close_menu, allocate_ui_at_rect).
Core navigation, Miller/list/grid views, tabs, bookmarks, command-frame file operations, drop-folder workflow, theme presets/editor, privilege helper integration, indexed search, file previews, and the full search settings UI are implemented.
Search diagnostics now track last progress timestamp and include a CLI indexer tool (`ottrin-index`) for command-line progress logs. Search settings UI has updated hierarchy, reduced borders, and added schedule detection for locate/updatedb.
Indexer now builds the initial index before registering filesystem watchers to avoid stalls during watcher setup; indexing progress logs are visible in `ottrin-index`.
Indexer watchdog now auto-restarts if indexing progress stalls, and the Search settings status card shows watcher setup/health.
Search settings now estimate indexing progress using the previous scan count and reduce redundant indexing labels; summary strip is hidden on the Search tab. Appearance settings layout now stacks in compact widths.
Indexing indicators are now confined to the Search settings page; the status card shows a moving spinner, file counts, and percent estimate (when available), and the sidebar/summary indicators are removed.
Search status card layout is simplified to a single progress block (counts + percent + bar), removing the extra Status/Running/Auto-refresh columns to prevent overflow.
Index status UI now shows a single summary block: Status line, `current/total` with percent, active path line, and a progress bar while indexing. Ready shows a green dot with 100%.
Dev runs now auto-build and wire the privileged helper (unless OTTRIN_PRIV_HELPER or OTTRIN_DEV_HELPER_AUTOBUILD=0 is set).
Search indexing now starts when the Search settings tab opens, and the status line shows the most recent indexed file path during scans.
Index status now pins the path line to a fixed height and moves the percent into the status row to avoid layout shifts.
Path line is now single-line with truncation and hover tooltip to avoid stretched/justified text.
Path line now uses middle-ellipsis with left alignment to prevent bouncing while keeping full path on hover.
Path line is now painted directly at the left edge to eliminate layout-driven horizontal bouncing.
Packaging doc now explicitly states no-cargo runtime requirement and Windows installer targets (.exe now, MSIX later).
Index status percent now uses a plocate/locate count estimate when no prior full scan exists, and the horizontal progress bar is removed.
System locate DB age is now shown in Search settings, with a one-click option to install a daily updatedb cron job via pkexec when no system schedule is detected.
Indexing walk now uses `same_file_system(true)` to avoid crossing into FUSE/network mounts when scanning a root like `/home`, preventing hangs on slow mounts.
Search roots now auto-include mount points under the chosen roots (Linux) so FUSE mounts like pCloud get indexed as separate roots while still keeping scans safe.
Search DB foundation now uses SQLite: schema + migration module in `ottrin-search`, batch writes during indexing, load-from-DB on startup, and incremental updates applied to the SQLite index (legacy JSON cache retained as fallback).
Indexer now keeps the previous in-memory index active while a rebuild runs, swapping to the new index only once the scan completes.
SQLite scan checkpoints are persisted per root so scans can resume after restart; missing cursors trigger a full rewalk of that root.
Search settings now include an optional content indexing scope (enable toggle + include/exclude folders) to prepare for future full-text content extraction.

Search settings overhaul (v0.2 Phase 1B/1C) is complete:
- Live index status card with file count, progress during scan, last-scan timestamp
- Framed editable lists for indexed locations, excluded folders, exclude/include globs
- Default exclude patterns (editor scratch files, VCS dirs, Python cache, Trash)
- Platform-default exclude roots (Linux: /proc /sys /dev /run) — removable by user
- Supplemental results: plocate and private locate.db (updatedb) integration
- Advanced section: daily updatedb cron installer (pkexec) + fanotify privileged daemon option
- "Reset search settings to defaults" at page bottom — resets config only, does not clear index
- Settings viewport scrollbar fixed (non-floating, 8px, visible track)

ARCHITECTURE
Rust workspace with focused crates:
crates/ottrin-app for the native eframe entrypoint
crates/ottrin-ui for the egui application shell and all UI workflows
crates/ottrin-core for shared domain models, config, commands, and semantic file classification
crates/ottrin-platform for OS integration, file operations, trash/reveal, and privileged helper execution
crates/ottrin-search for indexed search, filesystem watching, and query fallback logic
crates/ottrin-preview for preview classification and preview loading
crates/ottrin-copy for transfer queue state models

IMPORTANT FILES
Cargo.toml
docs/AI_AGENT_RULES.MD
docs/ROADMAP.md
docs/PROJECT.md
crates/ottrin-app/src/main.rs
crates/ottrin-ui/src/lib.rs
crates/ottrin-core/src/lib.rs
crates/ottrin-platform/src/lib.rs
crates/ottrin-platform/src/bin/ottrin-priv-helper.rs
crates/ottrin-search/src/lib.rs
crates/ottrin-preview/src/lib.rs
crates/ottrin-copy/src/lib.rs

DECISIONS
The workspace is split into domain, UI, platform, search, preview, copy-queue, and app-entry crates rather than a monolith.
The UI is egui/eframe-based and keeps Miller columns as the primary navigation model.
Miller navigation uses Finder-style depth tracking with scroll freeze — see memory/miller_navigation.md for full spec.
Search is handled by a dedicated local indexing service with watcher-driven rebuilds and fallback querying while indexing.
Semantic file styling lives in shared/core concepts and is rendered centrally in the UI crate.
Privileged operations are routed through a helper-based architecture instead of baking elevation into normal file-operation paths.
Search engine never hardcodes exclusions — all excludes are user-controlled via SearchConfig (engine respects config only).
Settings window is a separate egui ViewportId; style/scroll settings must be applied to its ctx, not the main ctx.
`AI_STATE.md` is the authoritative operational state file and must be updated when roadmap status changes.

CURRENT ROADMAP
1. Search engine foundation — COMPLETE. SQLite + resumable scanning + <100ms query on 500k files.
2. Tandem View (dual-pane) — side-by-side tab display, pinning, independent navigation per side.
3. Theme editor redesign (click-on-preview to edit, import/export).
4. Space Preview → Quick Inspector (separate OS window, image/PDF/folder/media).

KNOWN ISSUES
Rename, Delete, Copy, Cut in context menu close the menu but don't execute (clipboard model exists, no execute binding yet).
Copy-to-target, Move-to-target in context menu are stubbed.
Tab drag-and-drop not implemented.
Multi-select not implemented (Ctrl+A selects last item only).
popup_below_widget / toggle_popup / close_popup(id) are deprecated in egui 0.33 (use egui::Popup instead) — works but flagged.
Preview overlay: text files show content; others show icon + "Open with default app" (no native open wired on all platforms yet).
Tandem View not yet implemented.

NEXT TASK
Tandem View (Phase 2A): data model and activation paths.

CONSTRAINTS
Always start from `docs/AI_AGENT_RULES.MD`.
Treat repository state and `AI_STATE.md` as authoritative, and verify docs against code when they conflict.
Do not rely on prior chat context for project state.
Do not create commits unless explicitly instructed.
Prefer the existing Rust workspace and egui architecture; avoid major architectural changes unless they fit the project goals.
Keep `AI_STATE.md`, `docs/ROADMAP.md`, and any other status files synchronized when project state changes.
