# Storage Locations status

Last reviewed: 2026-10-12

This source-level matrix separates implemented behavior from keyboard-visible
detail, native assistive integration, and physical-platform acceptance. It does
not claim a clean supported Linux desktop, physical attachment/removal, real
Windows drive/share discovery, removable-media acceptance, or packaged/manual
Move acceptance. Linux same-filesystem and cross-filesystem Move are implemented, including
identity-bound retained recovery and undo. Windows Move is now enabled for the
admitted same-volume/cross-volume NTFS cases; native FAT32 and unsupported
metadata boundaries still refuse safely. The detailed current implementation
and scoped native/package evidence are recorded in `ROADMAP.md`. This supersedes
the earlier source-only Move rejection status; it does not close full platform QA.

| ROADMAP requirement | Source implementation | Keyboard focus-visible detail | Native assistive stack | Physical/manual QA | Status |
| --- | --- | --- | --- | --- | --- |
| Discover user-accessible roots | `discover_storage_locations`; Linux mount parser/classifier; Windows `GetLogicalDrives`; bookmark reconciliation | Not applicable | Not applicable | No physical mount/removal or real Windows drive/share run | source implemented; physical QA pending |
| Compact collapsible Locations rail | `locations_rail_expanded`, `locations_rail_width`, `render_bookmarks_row` | Tiles are keyboard focusable | AccessKit is intentionally not enabled; no native assistive-stack claim | No packaged desktop workflow run | source implemented; native acceptance pending |
| Collapsed complete tiles, icons, and identity | `locations_rail_tile_text`, `storage_icon_for_kind`, bundled Material Symbols | `locations_rail_tile_detail` shows full name, mount path, and existing index state for pointer hover or keyboard focus | AccessKit remains excluded because its Unix AT-SPI event loop can abort when D-Bus disappears | No focused assistive/native run | source implemented; accessible-native integration and acceptance pending |
| Expanded order/selection parity and secondary path | One `bookmarks` iteration renders both states; same click navigation | Full detail remains available on focus or hover; expanded text includes mount path | No native claim | No native interaction run | source implemented; native acceptance pending |
| Material Symbols icon family | Bundled `MaterialSymbolsFilled.ttf`, `icons.rs`, semantic rail mapping | Not applicable | Not applicable | No focused visual prototype or native visual acceptance | source implemented; visual acceptance pending |
| Refresh and fail-safe unavailable current volume | Periodic refresh, availability reconciliation, unavailable-command rejection, explicit Home recovery | Not applicable | Not applicable | No physical attach/remove/map/disconnect run | source implemented; physical QA pending |
| Direct browsing with explicit independent indexing opt-in | `IndexedVolumeLocation`, active-root reconciliation, context-menu opt-in, scoped search roots | Index state appears in tile detail when it exists | No native claim | No removable-volume search workflow run | source implemented; physical QA pending |
| Cross-volume Copy and Move boundary | Transactional Copy; Linux same-filesystem/cross-filesystem Move with retained recovery/undo; admitted Windows NTFS Move with explicit dispositions and identity-bound recovery | Not applicable | Not applicable | Scoped Windows native and earlier packaged C:/Q: Overwrite evidence exists; physical removable and consolidated current-package acceptance remain open | See `ROADMAP.md` for exact artifacts and limits; a current Linux clipboard admission gap is reproduced below |

Focused source tests cover discovery, icon mapping, compact/expanded tile
contracts, focus-visible detail selection, refresh scheduling, unavailable
operation rejection, explicit index opt-in, cross-filesystem Copy, Linux
same-filesystem/cross-filesystem Move, and unsupported-volume/metadata refusal. They do not
replace the physical, packaged, and native acceptance gates listed above.

## Current Linux clipboard path-admission gap

On 2026-10-12, the exact stable AppImage (SHA-256
`9c0c353435ea5fb640616c6ac3caf1c3d44ee7dd42c675f4e792d85de0483efb`)
was launched in private Xvfb/DBus with a disposable Home/profile. Ctrl+C on a
selected ordinary file outside that Home failed before clipboard capture:
“Copy was not started: the storage identity for … cannot be proven”. Navigating
to a second local filesystem and pressing Ctrl+V produced no destination. Source
bytes were unchanged. The fixtures were two distinct tmpfs instances (`st_dev`
39 and 30), not UUID-backed removable storage; this is refusal evidence, not
successful cross-volume Copy acceptance.

The causal source is `capture_required_storage_bindings`: clipboard capture
accepts only Home or a discovered root with stable identity. Linux discovery
currently exposes `/media`, `/mnt` and `/run/media` mounts, excluding tmpfs.
Other operation admission uses `operation_storage_bindings`, which omits unknown
paths instead of returning that clipboard error. Local path admission and queued
operation provenance need one coherent policy. Repair must preserve the existing
missing/replaced-volume refusal and must not authorise arbitrary pasted paths or
assign one parent-volume identity to a different mounted filesystem beneath it.

Receipt: `.hermes/evidence/linux-cross-volume-20261012/RESULT.json`. The app exited
cleanly; its private display, bus, index daemon, extraction/profile and both
filesystem fixtures were removed, as recorded in `exit.json` and `cleanup.json`
beside that receipt. Native removable-volume acceptance remains separate.

### Source repair — 2026-10-12

Clipboard and queued commands now use one fallible admission path. Every Linux
source/destination captures its unique current mount ID; discovered volume
identity remains an additional check, including for nested mounts. Home no
longer bypasses actual mount validation. Sources bind a final symlink's own
filesystem and directory destinations bind its target filesystem; missing paths
re-probe the nearest existing ancestor, and dangling followed links are refused.
New clipboard capture also preserves unavailable-volume tombstones.

Source verification passes 341 UI tests, three focused platform tests, and strict
Linux/Windows UI/platform all-target Clippy. The explicit private-namespace native
fixture passes actual tmpfs detach/replacement with unchanged parent identity and
no copied replacement bytes. Unique mount IDs require Linux 6.8+ and are never
persisted as index/reconnect identities. The exact rebuilt-package acceptance
below follows this source verification; the earlier refusal receipt remains
the historical reproduction.

### Exact AppImage acceptance — 2026-10-12

Clean source `898e42c0fb7a5cb166408de04a3e008f6df1b8d5` produced AppImage
SHA-256 `e2cc574af6eeaa4a6c57b89d40079354236ae210bf3bf22585bcf451ec9f1dc1`.
Private Xvfb/DBus GUI checks on two distinct tmpfs instances outside Home pass
Copy, conflict auto-renaming, two-file reverse Copy, Move, cancellation and
overwrite Move. Overwrite retains the exact displaced destination bytes; its
Activity Undo restores both source and displaced destination bytes. The app
exits normally with code zero; all owned profiles, fixtures and processes are removed.
Receipt: `dist/packages/LOCAL_PATH_ADMISSION_ACCEPTANCE.json`. This is not
physical removable-volume or full daily-driver acceptance.

The same run exposes two remaining issues: fast Ctrl+L can miss a modifier
released before the frame boundary, and successful batch Copy retains private
transaction stages with a cleanup warning and no batch Undo. Path navigation
used the real pointer address control for the recorded file-operation checks.
An earlier interactive run reached its own QA deadline before final overwrite
verification; the bounded rerun above supplies that verification.

### Rapid command shortcuts and refreshed package — 2026-10-12

The first shortcut candidate (`114abcd`, SHA-256
`431970b6024d0adf51e78f38455fd8b7f42b14888caa22498a0eee30273d00ed`)
passed rapid path entry but exposed the same frame-boundary modifier miss in
Select All. That failed workflow and its completed cleanup remain in
`.hermes/evidence/address-shortcut-20261012/first-attempt/RESULT.json`.

Main command shortcuts now read modifiers from the key press event. Address
fields request focus before rendering, so typing in the opening frame reaches
the field. Both regressions, all 343 ordinary UI tests and strict Linux/Windows
UI all-target Clippy pass. Clean `de83a2e` AppImage SHA-256
`b596304dfe6e8f9451792c411ffaffd4e6daa66a42adaf12e9f116182dd6ea68`
passes repeated rapid Ctrl+L/Ctrl+A/path typing without an inserted focus wait,
two-file selection and the complete Copy/Move/Cancel/Overwrite/Undo byte checks
recorded above, with normal exit and full owned fixture/process cleanup.
`dist/packages/LOCAL_PATH_ADMISSION_ACCEPTANCE.json` now binds this exact artifact.
Batch Copy transaction retention/Activity handling and physical removable-volume
acceptance remain open; Windows packages still need the current UI repair.

### Batch Copy receipt and Undo acceptance — 2026-10-12

Clean source `771eae8dc2bcd1268403bedc3dedf7f24778e611` produced AppImage
SHA-256 `312eb7a0907e3895aee45ef5c087be315c5a6d94f6f70d44f3a3cff7b237a7c7`.
Its private GUI run passes two-file cross-filesystem Copy and batch Undo: both
copied destination entries disappear and both original sources retain exact
bytes. Activity offers “Undo copied items” without the prior cleanup warning.
The rapid path-entry, Copy conflict rename, Move, Cancel, overwrite retention
and overwrite Undo checks pass again; normal exit and complete owned teardown
pass. Receipt: `dist/packages/BATCH_COPY_ACCEPTANCE.json`.

All 329 platform and 343 regular UI tests, plus strict Linux/Windows UI/platform
all-target Clippy, pass. Source-built worker coverage also proves overwrite
restoration, pathname-replacement refusal and retry without repeating a completed
Undo. The shared batch loop retains completed-prefix receipts and recovery paths
on a later Copy failure. GUI partial-failure acceptance and native Windows Copy
acceptance remain separate.

Retained Copy transaction data has an Activity owner. History trimming and
“Clear finished” now preserve running operations and all records that still own
Undo receipts, including completed or unavailable Copy Undo receipts with retained
transaction data. Ordinary completed/failed history remains discardable. A real
overwrite Copy regression survives 150 subsequent records and clearing, then
restores the original destination bytes through Undo; an all-running history can
exceed 100 records without losing results. All 348 regular UI tests and strict UI
Clippy pass on Linux and for the Windows target. The exact AppImage from clean
revision `89fa193`, SHA-256
`b8d37746dcc0ce49e48b9c68c791fef3f98c96ae57c1bdbdd48530655af3d7db`,
passes Dark 100% and Light 150%: clear finished Trash history, retain Copy Undo,
then remove the copied destination without changing the source. Both launches
exit normally and all private processes/profiles are removed. Portable, DEB and
RPM candidates are refreshed and their executables/renderer/source identities
pass the production package comparison. Receipt:
`dist/packages/ACTIVITY_RETENTION_ACCEPTANCE.json`. Current Windows packages
remain at the preceding revision; native installation acceptance is separate.

Safe expiry/reclamation and durable Copy recovery ownership across application
restarts remain open. Keeping session records does not implement those outcomes.
The earlier successful-Undo error-ink defect has been corrected; its screenshot
is historical.
