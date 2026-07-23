# Ottrin Roadmap

Last updated: 2026-07-23

Ottrin is in v1 release hardening. The product focus is daily-driver reliability: navigation, file operations, search, previews, packaging, and clear failure states.

## Current Status

Implemented:

- Miller, List, and Grid views.
- View Options popover for current view mode, thumbnail toggles, hidden files, sorting, and list columns.
- Tabs, bookmarks, path navigation, and keyboard-first movement.
- Tandem View with side-by-side panes and pane-to-pane copy.
- Multi-select for current-folder selections.
- File operations: copy, rename, trash, permanent delete, create file/folder, and command-frame actions. Move mutation remains disabled until the handle-owned, journaled protocol in [Durable Move Recovery Design](MOVE_RECOVERY_DESIGN.md) is implemented and proven per platform and conflict mode.
- Context menus with default open, open-with-application, folder location, path copy, rename/delete, bookmarks, and copy destinations.
- Smart panel sections for Info, Drop Folder, Search, Activity, and Trash.
- Cross-platform Trash panel implementation with Linux Freedesktop trash support and Windows Recycle Bin backend.
- SQLite-backed indexed search with watcher updates, background updater scheduling, generated-folder skips, result actions, content search snippets, and column resizing.
- Quick Inspector as a separate Space-triggered viewport with image, text/code, PDF text/metadata, DOCX content, XLSX/XLSM worksheet tables, PPTX slide text, archive, folder, and generic file inspectors.
- Theme presets and custom theme editing.
- Linux portable tarball, Linux AppImage, Windows portable zip, and per-user Windows installer packaging scripts.
- Strict formatting, tests, and Clippy gates.

Still needs real QA before v1:

- Linux manual workflow QA across navigation, file operations, search, Tandem View, multi-select, Quick Inspector, context menus, thumbnail view options, and status messages.
- Linux privileged helper and polkit smoke test from packaged layout.
- Real Windows runtime smoke for portable zip: launch, navigation, file operations, Trash/Recycling Bin, search, Quick Inspector, UAC/helper behavior, and background updater.
- Real Windows install/uninstall smoke for the installer candidate.
- Final release notes and known limitations review.

Move re-enablement is deliberately separate from ordinary workflow QA. Its journal, recovery, crash-test, and platform gates are defined in [Durable Move Recovery Design](MOVE_RECOVERY_DESIGN.md); no mode is enabled merely because another mode or platform passes.

## V1 Release Gate

V1 should not ship until:

- `cargo fmt --check`, `cargo test --workspace`, and strict Clippy pass.
- Linux and Windows CI pass.
- Linux AppImage and portable tarball launch from clean artifacts.
- Windows portable zip launches and passes real desktop smoke testing.
- Privileged helper behavior is validated on Linux and Windows.
- Known limitations match the actual tested state.

## Deferred Work

These are useful ideas, but they are not v1 blockers unless QA proves otherwise:

- Text/code editing in Quick Inspector.
- Multi-page PDF navigation and faithful Office layout rendering.
- Rich file thumbnails beyond images: generated text/code preview cards, rendered PDF first-page thumbnails, video frame thumbnails, and Office/document thumbnails where platform support or dependencies make sense.
- Recursive folder size thumbnails and folder thumbnail collages.
- Native desktop application discovery for richer Open With menus.
- Media playback and advanced image tools.
- True FTS5 content index.
- Fanotify helper/service packaging.
- Tab drag-and-drop.
- Rubber-band selection.
- Full undo for file operations.
- Native Linux `.deb`/`.rpm`, Windows MSI, macOS packages, and code signing.
