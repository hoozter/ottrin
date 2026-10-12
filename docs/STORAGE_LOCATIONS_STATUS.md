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
