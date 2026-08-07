# Ottrin Roadmap

Last updated: 2026-08-02

Ottrin is in v1 release hardening. The product focus is daily-driver reliability: navigation, file operations, search, previews, packaging, and clear failure states.

## Current Status

Implemented:

- Miller, List, and Grid views.
- View Options popover for current view mode, thumbnail toggles, hidden files, sorting, and list columns.
- Tabs, bookmarks, path navigation, and keyboard-first movement.
- Tandem View with side-by-side panes and pane-to-pane copy.
- Multi-select for current-folder selections.
- File operations: copy, rename, trash, permanent delete, create file/folder, and command-frame actions. Linux same-filesystem Move now has source- and automation-level support for Skip/Rename/Overwrite/Cancel with explicit dispositions and native rename behavior. Cross-filesystem Move and Windows Move remain disabled without mutation; reopening either requires a separately accepted protocol and evidence for that platform and mode.
- Context menus with default open, open-with-application, folder location, path copy, rename/delete, bookmarks, and copy destinations.
- Smart panel sections for Info, Drop Folder, Search, Activity, and Trash.
- Cross-platform Trash panel implementation with Linux Freedesktop trash support and Windows Recycle Bin backend.
- SQLite-backed indexed search with watcher updates, background updater scheduling, generated-folder skips, content search snippets, column resizing, click selection, double-click/Enter opening, shared right-click actions, and Space-triggered full preview.
- Quick Inspector as a full Space-triggered window with visual image, bounded multi-page PDF previews with Previous/Next/go-to-page navigation, bounded DOCX/XLSX/XLSM/ODS content-preview PNGs, and a Unix-only bounded first-slide PPTX text card; it retains extracted PDF/Office text and metadata fallbacks. Complete direct UTF-8 text previews can be edited on Linux through the fail-closed atomic replacement path; derived, truncated, linked, metadata-bearing, sparse, and unsupported-platform sources remain read-only. The PPTX card is non-authoritative. Office output is a truthful content preview, not a faithful document layout.
- Grid rubber-band selection is implemented and source-tested; packaged/manual acceptance is pending.
- Linux native Open With discovery and launch path is implemented with source and automated coverage; other platform discovery remains fallback/manual.
- Generated visual text/source cards, bounded direct-child folder thumbnail collages, bounded recursive folder-size metadata, and bounded PDF page navigation are implemented and test-covered.
- Theme presets and custom theme editing.
- Linux portable tarball, Linux AppImage, Windows portable zip, and per-user Windows installer packaging scripts.
- Windows portable and installer packaging is designed to retain native WGPU as the Automatic fallback and include a checksum-pinned Mesa 26.1.3 OpenGL compatibility runtime, app-local loader/driver payload, renderer preferences, diagnostic override, and complete notices. These remain exact-artifact verification and native-runtime gates; no current package acceptance is claimed.
- Strict formatting, tests, and Clippy gates.

## Still needs implementation or real QA before v1:

Storage Locations have source-level evidence in the tracked
[Storage Locations status](STORAGE_LOCATIONS_STATUS.md). Keyboard focus now
requests the same full location detail as pointer hover; however, AccessKit is
intentionally excluded while its Unix AT-SPI event loop can abort after a D-Bus
endpoint disappears. Native assistive integration and physical/manual
acceptance, including real attachment/removal and real Windows drive/share
coverage, remain v1 work. Source tests do not satisfy those gates.

- Storage Locations must expose every user-accessible filesystem root instead of assuming the home profile is the whole machine. Windows must discover and navigate fixed, removable, optical, and mapped-network drive letters such as `D:` and `P:`; Linux must discover user-visible mounted volumes and removable-media mount points.
- Creating an empty file or folder must be a first-class V1 workflow, not a command-frame shortcut disguised as a dialog. New File and New Folder actions must open a cohesive modal for the displayed directory, preserve spaces and Unicode, reject empty/reserved/traversal/separator names, never overwrite an existing entry, support Enter to create and Escape to cancel, refresh the correct tab, and select the new item after success. The actions must be discoverable from the main menu and blank-folder context menu; the command frame remains a power-user path with equivalent name handling.
- The Locations UX must keep Ottrin visually clean while making Home, bookmarks/quick links, drives, and mounted volumes immediately discoverable. Use a compact collapsible Locations rail rather than requiring a permanently wide second sidebar.
- The collapsed rail remains a complete, genuinely narrow navigation surface and must visually belong to the Smart Panel family: standard-size semantic icons with matching panel surface/selection treatment, no letters, slash, drive label, or folder name beneath the icon, and no empty gutter between the rail and file view. Home, root/storage, Desktop, Downloads, Documents, removable, optical, network, and unavailable locations use familiar distinct symbols rather than generic folders. The expand/collapse control uses a panel affordance rather than a navigation arrow. Each tile is keyboard-focusable; hover/focus exposes the full accessible label and mount path, and identity must never depend on color alone.
- The expanded rail shows the same items without changing order or selection, using the volume/share name as the primary label and the drive letter or mount point as secondary text. Expansion adds context, not functionality; selecting a location behaves identically in either state.
- Keep the current Material Symbols family until a focused, side-by-side icon prototype proves a replacement clearer at Ottrin's real 16/20/24 px sizes. Follow the [Icon System Evaluation](ICON_SYSTEM_EVALUATION.md): evaluate selected Tabler SVGs before any broad migration and do not adopt a whole icon font merely for coverage. Filesystem labels (`NTFS`, `ext4`, `Btrfs`, and similar) and semantic device state are preferable to using an operating-system logo as a filesystem guess, while removable, optical, network, unavailable, and system-volume identities should remain visually distinct.
- Drive/volume discovery must refresh after attachment, removal, mapping, or disconnection. An unavailable or removed current volume must produce a clear non-destructive state and a safe navigation route; it must never silently redirect a file operation.
- External-volume navigation must work without automatically indexing every attached drive. Users choose which additional volumes enter indexed search; direct browsing and scoped fallback search remain available independently.
- Copy must be tested across volume boundaries, including removable drives. Cross-filesystem Move remains rejected without mutation and may not degrade into an unjournaled copy-then-delete operation.
- Linux manual workflow QA across navigation, file operations, search, Tandem View, multi-select, Quick Inspector, context menus, thumbnail view options, and status messages.
- Linux privileged helper and polkit smoke test from packaged layout.
- Real Windows runtime smoke for portable zip: launch, navigation, file operations, Trash/Recycling Bin, search, Quick Inspector, UAC/helper behavior, and background updater.
- Real Windows install/uninstall smoke for the installer candidate.
- Release-owner review of release notes and known limitations against the exact artifacts.

Move enablement is per platform and mode. Accepted Linux same-filesystem native rename does not enable cross-filesystem or Windows Move. The frozen [Durable Move Recovery Design](MOVE_RECOVERY_DESIGN.md) records one possible future source-claim protocol, not a prerequisite for the accepted same-filesystem path.

## V1 Release Gate

V1 should not ship until:

- `cargo fmt --check`, `cargo test --workspace`, and strict Clippy pass.
- Linux and Windows CI pass.
- Linux AppImage and portable tarball launch from clean artifacts.
- New File and New Folder pass native Linux and Windows workflows with spaces, Unicode, invalid-name and existing-entry rejection, correct post-create selection, and no overwrite.
- The collapsed and expanded Locations rail pass native visual, pointer, keyboard, tooltip, and selection-parity review alongside the Smart Panel.
- Windows portable zip launches and passes real desktop smoke testing.
- Windows secondary fixed drives, removable drives, and mapped network drives are discoverable and navigable; Linux user-visible mounted/removable volumes pass the equivalent attach, browse, detach, and unavailable-state checks.
- Cross-volume Copy passes. Cross-volume Move remains disabled without mutation unless a separately accepted protocol later passes its platform-specific safety, hot-unplug, packaged, and native acceptance gates.
- Privileged helper behavior is validated on Linux and Windows.
- Known limitations match the actual tested state.

## Deferred Work

These are useful ideas, but they are not v1 blockers unless QA proves otherwise:

- Advanced Office navigation/editing or faithful Office layout beyond the bounded content preview required for v1.
- Rich file thumbnails beyond the implemented text-card and thumbnail-collage path remain deferred: video frame thumbnails and broader document variants where platform support or dependencies make sense. Video thumbnails now have an approved native-only Phase 1: Windows Media Foundation first, no bundled/invoked/recommended/downloaded FFmpeg, GStreamer codec payload, codec pack, external decoder, or additional codec implementation; unapproved platforms remain metadata-only. [Video Thumbnail Backend Spike](VIDEO_THUMBNAIL_SPIKE.md) preserves the rejected FFmpeg-first evaluation as historical evidence.
- Native desktop Open With discovery parity beyond Linux remains deferred.
- Media playback and advanced image tools.
- Fanotify helper/service packaging.
- Phase 2 continuous indexing: an always-on per-user index daemon with instant attach/reconciliation is the intended replacement for the v1 in-process watcher plus scheduled full refresh. It is not implemented or claimed by the current background updater.
- Tab drag-and-drop is implemented with source and automated tests; packaged/manual acceptance remains pending.
- List rubber-band selection is implemented with shared Grid semantics and source tests; packaged/manual acceptance remains pending.
- Full undo for file operations. Linux has an initial session-only undo action for one completed
  same-directory Rename: it rechecks the renamed link entry immediately before the same atomic
  no-clobber rename used forward. Linux has no identity-conditional rename, so this is a
  fail-closed stale-entry check with a residual external mutation TOCTOU window, not an
  adversarial-race guarantee; undo for all other operations remains deferred.
- Native Linux `.deb`/`.rpm`, Windows MSI, macOS packages, and code signing.
