# Ottrin Roadmap

Last updated: 2026-08-11

Ottrin is in v1 release hardening. The product focus is daily-driver reliability: navigation, file operations, search, previews, packaging, and clear failure states.

## Current Status

Implemented:

- Miller, List, and Grid views.
- View Options popover for current view mode, thumbnail toggles, hidden files, sorting, and list columns.
- Tabs, bookmarks, path navigation, and keyboard-first movement.
- Tandem View with side-by-side panes and pane-to-pane copy.
- Multi-select for current-folder selections.
- File operations: copy, rename, trash, permanent delete, create file/folder, and command-frame actions. Linux same-filesystem Move now has source- and automation-level support for Skip/Rename/Overwrite/Cancel with explicit dispositions and native rename behavior. Cross-filesystem Move and Windows Move remain disabled without mutation; reopening either requires a separately accepted protocol and evidence for that platform and mode.
- First-class New File and New Folder dialogs target the displayed directory, preserve spaces and Unicode, reject unsafe/reserved/existing names without overwrite, support Enter/Escape, refresh the originating tab, and select the created item only after success. Main-menu and folder-context entry points share the same validated command path; native Linux and Windows workflow acceptance remains pending.
- Context menus with default open, open-with-application, folder location, path copy, rename/delete, bookmarks, and copy destinations.
- Smart panel sections for Info, Drop Folder, Search, Activity, and Trash.
- Cross-platform Trash panel implementation with Linux Freedesktop trash support and Windows Recycle Bin backend.
- SQLite-backed indexed search with a Linux per-user daemon, watcher updates independent of GUI windows, generated-folder skips, content search snippets, column resizing, click selection, double-click/Enter opening, shared right-click actions, and Space-triggered full preview. Other platforms retain their current process/scheduler behavior until their daemon lifecycle is implemented and verified.
- Quick Inspector as a full Space-triggered window with visual images, bounded layout-rendered multi-page PDF previews with Previous/Next/go-to-page navigation, format-aware spreadsheet grids, and bounded Office metadata/text extraction. Extracted Office text is explicitly labeled, left aligned, and presented only as a fallback; page-shaped PNGs generated from flattened Office XML are not presented as source-layout previews. Complete direct UTF-8 text previews can be edited on Linux through the fail-closed atomic replacement path; derived, truncated, linked, metadata-bearing, sparse, and unsupported-platform sources remain read-only.
- Grid rubber-band selection is implemented and source-tested; packaged/manual acceptance is pending.
- Linux native Open With discovery and launch path is implemented with source and automated coverage; other platform discovery remains fallback/manual.
- Generated visual text/source cards, bounded direct-child folder thumbnail collages, bounded recursive folder-size metadata, and bounded PDF page navigation are implemented and test-covered.
- Windows video thumbnails have a source-complete native-only Media Foundation Phase 1 path. It admits a no-follow regular file by retained handle, decodes at most one bounded frame, normalizes pixels before generation-scoped cache/UI admission, and falls back to metadata on every unavailable/error path. Linux and other platforms remain metadata-only; native Windows and exact-package acceptance are still pending.
- Theme presets and custom theme editing.
- A compact collapsible Locations rail is implemented with stable item ordering and selection across compact/expanded modes, Smart Panel surface treatment, semantic location icons, keyboard focus/tooltips, discovered storage reconciliation, and no automatic indexing of attached volumes. Source layout tests cover compact width, shared surface, and input-boundary leakage; native visual and device-lifecycle acceptance remains pending.
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
- New File and New Folder are source-implemented and focused tests cover targeting, spaces, Unicode, unsafe/reserved/existing-name rejection, no-overwrite command construction, compact controls, and post-success selection. Native Linux and Windows workflows still need to confirm Enter/Escape, main-menu and blank-folder discovery, correct-tab refresh/selection, and rendered modal behavior.
- The compact and expanded Locations rail is source-implemented and tested for compact width, Smart Panel surface continuity, and pointer-input boundaries. Native visual, pointer, keyboard, tooltip, stable-order/selection parity, attachment/removal/mapping refresh, and unavailable-volume behavior remain v1 QA gates.
- Keep the current Material Symbols family until a focused, side-by-side icon prototype proves a replacement clearer at Ottrin's real 16/20/24 px sizes. Follow the [Icon System Evaluation](ICON_SYSTEM_EVALUATION.md): evaluate selected Tabler SVGs before any broad migration and do not adopt a whole icon font merely for coverage. Filesystem labels (`NTFS`, `ext4`, `Btrfs`, and similar) and semantic device state are preferable to using an operating-system logo as a filesystem guess, while removable, optical, network, unavailable, and system-volume identities should remain visually distinct.
- Drive/volume discovery must refresh after attachment, removal, mapping, or disconnection. An unavailable or removed current volume must produce a clear non-destructive state and a safe navigation route; it must never silently redirect a file operation.
- External-volume navigation must work without automatically indexing every attached drive. Users choose which additional volumes enter indexed search; direct browsing and scoped fallback search remain available independently.
- Copy must be tested across volume boundaries, including removable drives. Cross-filesystem Move remains rejected without mutation and may not degrade into an unjournaled copy-then-delete operation.
- Linux manual workflow QA across navigation, file operations, search, Tandem View, multi-select, Quick Inspector, context menus, thumbnail view options, and status messages.
- DOCX and PPTX still need a sandboxed layout renderer that preserves pages, positions, tables, images, typography, and hierarchy. Until one is implemented and physically verified, their extracted text remains a clearly labeled fallback rather than a simulated document page.
- Linux privileged helper and polkit smoke test from packaged layout.
- Real Windows runtime smoke for portable zip: launch, navigation, file operations, Trash/Recycling Bin, search, Quick Inspector, UAC/helper behavior, and background updater.
- The Media Foundation thumbnail path still needs native Windows tests for retained-handle/reparse identity, cancellation/deadline behavior, malformed and unsupported media, installed-codec capability boundaries, source/resource limits, cache invalidation, and an exact-package audit proving that no decoder, codec payload, or installer was added. Cross-compilation and non-Windows contract tests are not native acceptance.
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

- Advanced Office navigation and editing beyond faithful read-only preview.
- Rich file thumbnails beyond the implemented text-card, thumbnail-collage, and Windows Media Foundation video path remain deferred where platform support or dependencies require a separate decision. The native-only video policy adds no bundled/invoked/recommended/downloaded FFmpeg, GStreamer codec payload, codec pack, external decoder, or additional codec implementation; unapproved platforms remain metadata-only. [Video Thumbnail Backend Spike](VIDEO_THUMBNAIL_SPIKE.md) preserves the rejected FFmpeg-first evaluation as historical evidence.
- Native desktop Open With discovery parity beyond Linux remains deferred.
- Media playback and advanced image tools.
- Fanotify helper/service packaging.
- Phase 2 continuous indexing is implemented on Linux as a same-user Unix-socket daemon with a single SQLite writer, concurrent GUI/CLI clients, live watcher reconciliation, persisted snapshot restart, explicit status/rebuild/configuration IPC, and a restartable systemd user-service installer. Packaged Linux and upgrade-migration acceptance remains an exact-artifact gate.
- Tab drag-and-drop is implemented with source and automated tests; packaged/manual acceptance remains pending.
- List rubber-band selection is implemented with shared Grid semantics and source tests; packaged/manual acceptance remains pending.
- Full undo for file operations. Linux has an initial session-only undo action for one completed
  same-directory Rename: it rechecks the renamed link entry immediately before the same atomic
  no-clobber rename used forward. Linux has no identity-conditional rename, so this is a
  fail-closed stale-entry check with a residual external mutation TOCTOU window, not an
  adversarial-race guarantee; undo for all other operations remains deferred.
- Native Linux `.deb`/`.rpm`, Windows MSI, macOS packages, and code signing.
