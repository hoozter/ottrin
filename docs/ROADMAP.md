# Ottrin Roadmap

Last updated: 2026-07-29

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
- SQLite-backed indexed search with watcher updates, background updater scheduling, generated-folder skips, content search snippets, column resizing, click selection, double-click/Enter opening, shared right-click actions, and Space-triggered full preview.
- Quick Inspector as a full Space-triggered window with visual image, bounded first-page PDF previews, and bounded DOCX/XLSX/XLSM content-preview PNGs; it retains extracted PDF/Office text and metadata fallbacks. Office output is a truthful content preview, not a faithful document layout.
- Theme presets and custom theme editing.
- Linux portable tarball, Linux AppImage, Windows portable zip, and per-user Windows installer packaging scripts.
- Windows portable and installer artifacts retain native WGPU as the Automatic fallback and bundle a checksum-pinned Mesa 26.1.3 OpenGL compatibility runtime, including the app-local `opengl32.dll` loader and its `libgallium_wgl.dll` driver bundle. Automatic selects the bundled compatibility path for detected RDP sessions and recognized virtual/problem adapters, explains that one-time choice in the app, and exposes Automatic, Native, and OpenGL compatibility preferences for the next launch. `ottrin.exe --renderer opengl` or `OTTRIN_RENDERER=opengl` remains a diagnostic override; the bundled Mesa, pal1000, and Microsoft notices are under `third-party-notices/`.
- Strict formatting, tests, and Clippy gates.

## Still needs implementation or real QA before v1:

- Storage Locations must expose every user-accessible filesystem root instead of assuming the home profile is the whole machine. Windows must discover and navigate fixed, removable, optical, and mapped-network drive letters such as `D:` and `P:`; Linux must discover user-visible mounted volumes and removable-media mount points.
- The Locations UX must keep Ottrin visually clean while making Home, bookmarks/quick links, drives, and mounted volumes immediately discoverable. Prototype a compact collapsible Locations rail or equivalent unified surface before committing to a permanently wide second sidebar.
- Drive/volume discovery must refresh after attachment, removal, mapping, or disconnection. An unavailable or removed current volume must produce a clear non-destructive state and a safe navigation route; it must never silently redirect a file operation.
- External-volume navigation must work without automatically indexing every attached drive. Users choose which additional volumes enter indexed search; direct browsing and scoped fallback search remain available independently.
- Copy and Move must be tested across volume boundaries, including removable drives. Cross-filesystem Move remains gated by the durable Move protocol and may not degrade into an unjournaled copy-then-delete operation.
- Linux manual workflow QA across navigation, file operations, search, Tandem View, multi-select, Quick Inspector, context menus, thumbnail view options, and status messages.
- Linux privileged helper and polkit smoke test from packaged layout.
- Real Windows runtime smoke for portable zip: launch, navigation, file operations, Trash/Recycling Bin, search, Quick Inspector, UAC/helper behavior, and background updater.
- Real Windows install/uninstall smoke for the installer candidate.
- Release-owner review of release notes and known limitations against the exact artifacts.

Move re-enablement is deliberately separate from ordinary workflow QA. Its journal, recovery, crash-test, and platform gates are defined in [Durable Move Recovery Design](MOVE_RECOVERY_DESIGN.md); no mode is enabled merely because another mode or platform passes.

## V1 Release Gate

V1 should not ship until:

- `cargo fmt --check`, `cargo test --workspace`, and strict Clippy pass.
- Linux and Windows CI pass.
- Linux AppImage and portable tarball launch from clean artifacts.
- Windows portable zip launches and passes real desktop smoke testing.
- Windows secondary fixed drives, removable drives, and mapped network drives are discoverable and navigable; Linux user-visible mounted/removable volumes pass the equivalent attach, browse, detach, and unavailable-state checks.
- Cross-volume Copy passes, and each enabled cross-volume Move mode passes its platform-specific journal, crash-recovery, hot-unplug, and native packaged acceptance gates.
- Privileged helper behavior is validated on Linux and Windows.
- Known limitations match the actual tested state.

## Deferred Work

These are useful ideas, but they are not v1 blockers unless QA proves otherwise:

- Text/code editing in Quick Inspector.
- Multi-page PDF navigation and advanced Office navigation/editing or faithful Office layout beyond the bounded content preview required for v1.
- Rich file thumbnails beyond the common-document v1 preview gate: generated text/code preview cards, video frame thumbnails, and broader document thumbnails where platform support or dependencies make sense.
- Recursive folder size thumbnails and folder thumbnail collages.
- Native desktop application discovery for richer Open With menus.
- Media playback and advanced image tools.
- True FTS5 content index.
- Fanotify helper/service packaging.
- Tab drag-and-drop.
- Rubber-band selection.
- Full undo for file operations.
- Native Linux `.deb`/`.rpm`, Windows MSI, macOS packages, and code signing.
