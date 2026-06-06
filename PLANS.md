# Execution Plan

This file is the active queue. Keep it synchronized with `AI_STATE.md` and `docs/ROADMAP.md`.

## Phase 0 — Clean Baseline
**Status: Complete**

- [x] Verify current workspace with `cargo check`
- [x] Verify current workspace with `cargo test`
- [x] Verify release-mode search benchmark
- [x] Verify release build with `cargo build --release`
- [x] Remove stale build-failure notes from the repository
- [x] Replace stale root `ROADMAP.md` with a pointer to `docs/ROADMAP.md`
- [x] Update `AI_STATE.md`, `PLANS.md`, and `docs/ROADMAP.md` to reflect actual verified state
- [x] Expand CI workflow to run Linux/Windows checks, tests, release builds, and release-mode search benchmark
- [x] Make strict Clippy clean

## Phase 1 — Release Hardening Baseline
**Priority: Highest**

- [x] Add Linux CI: `cargo check`, `cargo test`, `cargo build --release`
- [x] Add Windows CI: `cargo check`, `cargo test`, `cargo build --release`
- [x] Add strict Clippy to CI
- [x] Verify local Windows GNU target check after installing `mingw-w64`
- [x] Verify local Windows GNU release build
- [ ] Confirm whether Windows-specific dependencies build cleanly on hosted CI after pushing
- [ ] Preserve release artifacts from CI for smoke testing
- [ ] Document exact supported Rust/toolchain version

## Phase 2 — Manual Workflow QA
**Priority: Highest**

### Linux
- [ ] Navigation smoke test: Miller/List/Grid, tabs, bookmarks, address bar
- [ ] Search smoke test: first index, rebuild, current folder, content search, result actions
- [ ] File operations smoke test: create, rename, copy, move, trash delete, permanent delete
- [ ] Tandem smoke test: activation, pinning, side switching, copy/move to opposite pane
- [ ] Preview smoke test: text, image, PDF/media/archive metadata fallback
- [ ] Privileged helper smoke test with polkit-installed helper

### Windows
- [ ] Build and launch app
- [ ] Navigation smoke test
- [ ] Search smoke test
- [ ] File operations smoke test
- [ ] Tandem smoke test
- [ ] Preview smoke test
- [ ] UAC helper smoke test

## Phase 3 — Packaging
**Priority: High**

- [ ] Linux portable/run-from-build artifact
- [ ] Linux AppImage
- [ ] Decide whether deb/rpm are needed before first public hardening release
- [ ] Windows portable zip
- [ ] Windows installer candidate
- [ ] Verify app runs without Cargo on both platforms
- [ ] Verify helper discovery from packaged layouts

## Phase 4 — Quick Inspector
**Priority: Medium**

- [ ] Replace current Space overlay with separate OS viewport
- [ ] Add close/toggle behavior with Escape and Space
- [ ] Implement richer image inspector
- [ ] Implement PDF page rendering and navigation
- [ ] Implement folder summary inspector
- [ ] Implement media metadata inspector
- [ ] Implement font/archive/unknown fallback inspectors

## Phase 5 — Usability Polish
**Priority: Medium**

- [ ] Multi-select model and UI
- [ ] Tab drag-and-drop
- [ ] Context-menu action/status QA and fixes
- [ ] Settings/cache cleanup
- [ ] Thumbnail-as-icon toggle
- [ ] Folder preview thumbnails

## Recently Completed
- Search overhaul through SQLite metadata storage and resumable scans
- Ripgrep-backed content search mode with snippets
- Tandem View through pane-to-pane copy/move actions
- Theme editor redesign through JSON import/export
