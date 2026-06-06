PROJECT
Name:
Ottrin

Purpose:
Keyboard-first graphical file manager with Finder-style Miller columns, integrated file operations, indexed search, previews, theming, tandem panes, and privilege-aware retry flows.

Main technologies:
Rust
egui / eframe 0.33
serde
notify
walkdir
rusqlite

VERIFIED BASELINE
Verified on 2026-06-06 from `/home/campbell/Projects/Ottrin` on Linux:
- `cargo check` passes.
- `cargo test` passes.
- `cargo test -p ottrin-search --release -- bench_query_500k --nocapture` passes at 30ms when run without competing release builds.
- `cargo build --release` passes.
- Release binary builds at `target/release/ottrin-app`.
- `cargo clippy --workspace --all-targets -- -D warnings` passes.
- `cargo check --target x86_64-pc-windows-gnu` passes locally after installing `mingw-w64`.
- `cargo build --release --target x86_64-pc-windows-gnu -p ottrin-app -p ottrin-platform -p ottrin-search` passes locally.
- Windows runtime behavior, UAC helper flow, and installer behavior still need real Windows smoke testing.

CURRENT STATUS
Core navigation, Miller/list/grid views, tabs, bookmarks, command-frame file operations, drop-folder workflow, theme presets/editor, search settings, indexed search, previews, and privilege-helper plumbing are implemented.
Search has a SQLite metadata store, schema migration, load-from-DB startup path, batch writes during indexing, scan checkpoints, watcher-driven updates, and fallback querying while indexing.
Search performance has unit coverage for a 500k-item benchmark. Strict <100ms timing is enforced in release-mode benchmark runs; normal debug tests use a looser threshold to avoid timing jitter.
Search panel has optional ripgrep-backed content search with snippets. It handles literal multi-word queries across lines and honors query filters.
Tandem View is implemented: dual-pane central layout, draggable divider, per-side tab content and controls, activation via folder context action / Shift-click, active-side tracking, pinning, and copy/move to the opposite pane.
Theme editor redesign is implemented: separate OS viewport, preview-first layout, click/hover region selection, region-specific controls, JSON import/export, and wrapped/bare theme payload support.
Packaging strategy is documented, but release artifacts and installer smoke tests are not complete.

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
README.md
docs/AI_AGENT_RULES.MD
docs/ROADMAP.md
docs/RELEASE_CHECKLIST.md
docs/PACKAGING.md
docs/PRIVILEGED_QA.md
crates/ottrin-app/src/main.rs
crates/ottrin-ui/src/lib.rs
crates/ottrin-core/src/lib.rs
crates/ottrin-platform/src/lib.rs
crates/ottrin-platform/src/bin/ottrin-priv-helper.rs
crates/ottrin-search/src/lib.rs
crates/ottrin-search/src/db.rs
crates/ottrin-preview/src/lib.rs
crates/ottrin-copy/src/lib.rs

DECISIONS
The workspace is split into domain, UI, platform, search, preview, copy-queue, and app-entry crates rather than a monolith.
The UI is egui/eframe-based and keeps Miller columns as the primary navigation model.
Search engine exclusions come from `SearchConfig`; avoid hidden hardcoded ignore lists beyond platform defaults stored in config.
Privileged operations route through helper processes; the main UI process stays unprivileged.
Settings window and theme editor use separate egui viewports; style/scroll settings must be applied to the relevant viewport context.
`AI_STATE.md`, `PLANS.md`, and `docs/ROADMAP.md` must stay synchronized when roadmap status changes.

CURRENT ROADMAP
1. Clean baseline and truthful state docs — COMPLETE.
2. Release hardening — IN PROGRESS: Linux and Windows CI/build verification, manual workflow QA, packaging smoke tests.
3. Quick Inspector — NEXT FEATURE: replace Space overlay with a separate OS window, then add richer image/PDF/folder/media inspectors.
4. Usability polish: multi-select, tab drag-and-drop, context-menu QA, settings/cache cleanup, thumbnails.

KNOWN ISSUES / UNVERIFIED AREAS
- Windows build, UAC helper flow, and installer behavior are not verified locally.
- Linux polkit helper packaging and privileged-operation QA still need real system tests.
- Multi-select is not implemented.
- Tab drag-and-drop is not implemented.
- Space Preview is still an in-app overlay; Quick Inspector separate window is not implemented.
- Preview support is limited: text and image paths are stronger than PDF/media/archive/font rich inspectors.
- Context-menu file actions need a manual QA pass; tandem/drop-folder/current-folder copy/move actions are wired, but older docs reported Rename/Delete/Copy/Cut menu issues that must be rechecked in the app.
- Packaging is documented but no fresh AppImage, deb/rpm, Windows portable, or Windows installer artifact has been produced in this baseline.

NEXT TASK
Release-hardening baseline:
1. Push or otherwise run the updated CI workflow and confirm Linux/Windows hosted results.
2. Run manual Linux smoke QA for navigation, search, file operations, previews, tandem, and privilege helper.
3. Verify Windows build and UAC helper path on Windows or CI.
4. Produce and smoke-test first Linux and Windows artifacts.

CONSTRAINTS
Always start from `docs/AI_AGENT_RULES.MD`.
Do not rely on prior chat context for project state.
Do not create commits unless explicitly instructed.
Prefer the existing Rust workspace and egui architecture; avoid major architectural changes unless they fit the project goals.
Keep `AI_STATE.md`, `PLANS.md`, and `docs/ROADMAP.md` accurate when status changes.
