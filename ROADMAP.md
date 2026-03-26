# Ottrin – Roadmap

**Last updated:** 2026-03-26  
**Status:** v0.1.0 released (2026-03-26). Maintenance mode until clear ownership.

Ottrin is a keyboard-first graphical file manager built in Rust (egui/eframe). It provides Miller columns, list/grid views, tabs, integrated file ops, bookmarks, global search, file previews, theme system, and privilege escalation.

---

## Status Overview

### Completed ✓ (v0.1.0)
- Miller columns navigation
- List and grid views with icon previews
- Tabbed interface (multiple directories)
- Basic file operations (copy, move, rename, delete)
- Bookmark system
- Global search (filename-based, uses plocate + walkdir)
- File previews for common types (text, images)
- Theme system (dark/light, custom palettes)
- Privilege escalation helper (sudo integration)
- Cross-platform builds (Linux, macOS, Windows)
- Packaging: AppImage, DMG, MSI

### Known Issues / Backlog
- Search benchmark not met (<100ms for 500k files) — needs optimization
- Full-text content indexing not implemented
- "Current Folder" scope for live walk (index may be stale)
- Ctrl+F focus search from anywhere not fully polished
- Batch operations (multi-select rename) missing

---

## Milestones

### Milestone 1: Search Performance & Polish (v0.2.0)
- [ ] Implement incremental indexing with change notifications (notify + walkdir)
- [ ] Add full-text content indexing (basic, using ripgrep or similar)
- [ ] Meet search benchmark: <100ms for 500k-file dataset
- [ ] Improve global search UI: results grouped, preview pane, filters
- [ ] Ensure "Current Folder" scope always reflects live filesystem
- [ ] Bind Ctrl+F globally to open search, regardless of focused widget
- [ ] Add fuzzy matching (skim-like) for filenames

### Milestone 2: Advanced File Operations (v0.3.0)
- [ ] Multi-select batch rename (pattern-based)
- [ ] Bulk move/copy to destination with progress UI
- [ ] Undo/redo stack for file operations
- [ ] Archive creation (zip/tar) and extraction inline
- [ ] Compare directories (diff view)
- [ ] Integrate with external diff/merge tools
- [ ] Enhanced permission editing (chmod, chown with preview)

### Milestone 3: Ecosystem & Usability (v0.4.0)
- [ ] Plugin system for custom previewers and actions (Lua or WASM)
- [ ] VCS integration (git status badges, common actions: commit, push, diff)
- [ ] Cloud storage mounting (rclone integration) with offline cache
- [ ] Improved terminal integration (open terminal in current directory, run command on selection)
- [ ] Synchronized panes (mirror navigation across columns)
- [ ] Accessibility improvements (screen reader support, high-contrast themes)
- [ ] Comprehensive user documentation and tutorial videos

---

## Post-3.0 Ideas

- Network shares (SMB/NFS) browsing with reconnect persistence
- Embedded media player for audio/video preview
- File synchronization tool (like rsync front-end)
- Tag-based organization (like macOS Finder tags)

---

## Notes

- Rust edition 2024; keep dependencies minimal and stable.
- Target 60fps UI responsiveness; avoid blocking the main thread.
- Prefer native OS file dialogs and integrations when possible.
- Community contributions welcome once ownership clarified.
