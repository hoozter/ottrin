# Storage Locations status

Last reviewed: 2026-08-01

This source-level matrix separates implemented behavior from keyboard-visible
detail, native assistive integration, and physical-platform acceptance. It does
not claim a clean supported Linux desktop, physical attachment/removal, real
Windows drive/share discovery, removable-media acceptance, or packaged/manual
Move acceptance. Linux same-filesystem Move is enabled only at the source and
automation tier; cross-filesystem and Windows Move reject without mutation.

| ROADMAP requirement | Source implementation | Keyboard focus-visible detail | Native assistive stack | Physical/manual QA | Status |
| --- | --- | --- | --- | --- | --- |
| Discover user-accessible roots | `discover_storage_locations`; Linux mount parser/classifier; Windows `GetLogicalDrives`; bookmark reconciliation | Not applicable | Not applicable | No physical mount/removal or real Windows drive/share run | source implemented; physical QA pending |
| Compact collapsible Locations rail | `locations_rail_expanded`, `locations_rail_width`, `render_bookmarks_row` | Tiles are keyboard focusable | AccessKit is intentionally not enabled; no native assistive-stack claim | No packaged desktop workflow run | source implemented; native acceptance pending |
| Collapsed complete tiles, icons, and identity | `locations_rail_tile_text`, `storage_icon_for_kind`, bundled Material Symbols | `locations_rail_tile_detail` shows full name, mount path, and existing index state for pointer hover or keyboard focus | AccessKit remains excluded because its Unix AT-SPI event loop can abort when D-Bus disappears | No focused assistive/native run | source implemented; accessible-native integration and acceptance pending |
| Expanded order/selection parity and secondary path | One `bookmarks` iteration renders both states; same click navigation | Full detail remains available on focus or hover; expanded text includes mount path | No native claim | No native interaction run | source implemented; native acceptance pending |
| Material Symbols icon family | Bundled `MaterialSymbolsFilled.ttf`, `icons.rs`, semantic rail mapping | Not applicable | Not applicable | No focused visual prototype or native visual acceptance | source implemented; visual acceptance pending |
| Refresh and fail-safe unavailable current volume | Periodic refresh, availability reconciliation, unavailable-command rejection, explicit Home recovery | Not applicable | Not applicable | No physical attach/remove/map/disconnect run | source implemented; physical QA pending |
| Direct browsing with explicit independent indexing opt-in | `IndexedVolumeLocation`, active-root reconciliation, context-menu opt-in, scoped search roots | Index state appears in tile detail when it exists | No native claim | No removable-volume search workflow run | source implemented; physical QA pending |
| Cross-volume Copy and Move boundary | Transactional Copy; Linux same-filesystem Move uses native rename with explicit dispositions; cross-filesystem and Windows Move reject before mutation | Not applicable | Not applicable | No removable-media/native cross-volume or packaged Move acceptance | Copy and Linux same-filesystem Move source-tested; other Move modes fail closed |

Focused source tests cover discovery, icon mapping, compact/expanded tile
contracts, focus-visible detail selection, refresh scheduling, unavailable
operation rejection, explicit index opt-in, cross-filesystem Copy, Linux
same-filesystem Move, and cross-filesystem/Windows Move rejection. They do not
replace the physical, packaged, and native acceptance gates listed above.
