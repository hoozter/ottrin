# Appearance verification inventory

Accepted Linux AppImage source: `1b5602cca90fc5e6da215edd84190abf86f5ccf5`.
Stable deliverable: `dist/packages/Ottrin-0.1.0-x86_64.AppImage`.
SHA-256: `1ce4c4249d7232af20ff3935cc74310067fb13d21a5bea7e8cd8f9eae31bce7c`.

## Property scope and consumers

All 38 properties were individually edited through the development interface: intended consumer changes, unrelated consumer preservation, property reset, Save/editor reopen and persisted reset passed. The exact AppImage additionally passed native import, Save and process-restart preservation of all 38 values; shared-window pixel/corner checks; and a fresh content edit/reset/Cancel test. Individual property edits were not all repeated in the package. `individual-control-evidence-audit.json` accounts for every stored field without missing entries.

Evidence keys (under `.hermes/evidence/appearance-overhaul/`): **S** = `surface-controls-running-check.json`; **T** = `text-controls-running-check.json`; **I** = `state-controls-running-check.json`; **N** = `number-controls-running-check.json`; **F** = `file-palette-controls-running-check.json`. Each key records the live edit, scope/isolation, reset and persistence results above.

Colors exposes the selected element's roles and separately labels shared defaults. Typography, Shape, States and File colors explicitly describe their application-wide scope. The same `Colors::for_config`, semantic palette, themed button/input helpers and viewport frame renderer supply editor previews and production consumers. Named `custom_themes` records are the sole persisted custom palette; unsaved editor state is separate. Superseded palette caches and unreachable Properties UI were removed.

| Control | Stored value | Actual scope and consumers | Live verification |
| --- | --- | --- | --- |
| Content background | `background` | Browsing canvas / file views | S — pass |
| Panels and dialogs | `panel` | Shared panel and dialog fill | S — pass |
| Inputs / raised surfaces | `panel_raised` | Inputs, inspector surface, raised containers | S — pass |
| Title bar | `titlebar` | Window chrome | S — pass |
| Toolbar + tabs | `toolbar` | Navigation and tab shelf | S — pass |
| Locations rail | `bookmarks_bar` | Left bookmarked locations/storage rail only | S — pass |
| Smart panel | `smart_panel` | Info/search/target sidebar | S — pass |
| Status bar | `statusbar` | Bottom status bar only | S — pass |
| Borders and dividers | `border` | Shared dividers and outlines | T — pass |
| Window border | `window_border` | Shared viewport frames and dialog outlines | T — pass |
| Active & focus accent | `accent` | Active/focus/links | T — pass |
| Selection background | `selected_bg` | Selected rows and text selection | S — pass |
| Hover background | `hover` | Hovered rows and widget states | I — pass |
| Error & danger | `error` | Error feedback and destructive-action labels/icons | I — pass |
| Folder icon | `folder` | Default folder icons with semantic colors off | T — pass |
| Button background | `button_bg` | Default filled action buttons, with independent input/checkbox roles | S — pass |
| Button text | `button_text` | Normal action-button and menu labels; destructive actions use Error & danger | T — pass |
| Primary text | `text` | Shared primary interface labels | T — pass |
| Secondary text | `text_dim` | Shared supporting interface labels | T — pass |
| Muted text | `text_muted` | Shared low-emphasis interface labels | T — pass |
| Heading text | `text_heading` | Window titles, Appearance section headings, Settings page titles and panel headers | T — pass |
| Folder text | `text_folder` | Neutral folder labels only | T — pass |
| File text | `text_file` | Neutral file labels and default file icons | T — pass |
| Documents / other | `palette_soft_white` | Document and unclassified-file icons; neutral labels retain File text | F — pass |
| Folders | `palette_folder_blue` | Semantic directory icons and optional colored folder labels | F — pass |
| Links / audio | `palette_link_blue` | File links and audio; blended script roles | F — pass |
| Config / disk images | `palette_steel` | Configuration and disk image icons/labels; blended code roles | F — pass |
| Programs / sheets | `palette_green` | Executables and spreadsheets; blended script roles | F — pass |
| Presentations | `palette_orange` | Presentations, packages and Rust/code mixtures | F — pass |
| Images | `palette_purple` | Images, fonts and markup mixtures | F — pass |
| Fonts (blend) | `palette_pink` | Fonts blended with the image color | F — pass |
| PDF / video | `palette_red` | PDF and video file roles | F — pass |
| Archives | `palette_yellow` | Archives, packages and code roles | F — pass |
| Text size | `font_scale` | Interface typography, file labels and supporting row/chrome dimensions; formatted document contents keep their own preview sizing | N — pass |
| Window corners | `app_radius` | Shared root/settings/editor/inspector viewport frames and Ottrin dialog shells | N — pass |
| Surface corners | `border_radius` | Containers, tabs and popup corners | N — pass |
| Button corners | `button_radius` | Button role, explicit action buttons and Inspector controls; inputs use surface corners | N — pass |
| Window border thickness | `window_border_width` | Shared viewport frames and dialog outlines | N — pass |

## Actions and interaction

| Control/action | Intended scope and observed result | Evidence |
| --- | --- | --- |
| Element selection / live preview | Dedicated roles for title bar, navigation, locations, file view, selection, file labels/icons, sidebar, status, buttons and borders; shared defaults remain visibly separate. All corresponding individual consumer edits pass. | Individual control audit; final editor captures; shared-rendering regressions |
| Light / Dark / System | Built-ins follow chosen mode; System follows portal/native preference; named themes retain their saved mode/colors. Private portal Dark→Light→Dark, explicit-mode isolation and saved-theme isolation pass. | `button-and-system-running-check.json`; system observer regressions |
| Seven built-in presets | Every preset in both modes renders coherently in main, Settings, editor and Inspector. Exact package selected/persisted all fourteen combinations at native 100% and 150%; normal/selection/hover/focus/error/disabled states captured and reviewed. | `package-bundled-themes-scale-1.0-running-check.json`, `package-bundled-themes-scale-1.5-running-check.json` |
| Color files by type / Also color folder labels | Toggles repaint immediately and persist; folder-label toggle preserves icons and other file rows; canvas remains unchanged; reversal restores rendering. | `file-color-toggles-running-check.json` |
| Hex / compact RGBA picker | Valid RGB edits preserve alpha; invalid drafts retain applied color and show error. RGB, hue, saturation/brightness and opacity edits preserve all unrelated properties. Alpha zero retains RGB; 50% overlays blend correctly. Both display scales inspected. | `compact-picker-controls-running-check.json`; packaged invalid-hex captures |
| Property reset | Restores only the chosen base property, including exact persisted RGBA/dimensions. Exact package content reset also passes. | All five property receipts; `package-edit-reset-cancel-running-check.json` |
| Reset element colors | Restores selected element while preserving separately edited shared panel. | `reset-actions-running-check.json` |
| Reset entire theme to base | Restores all base values, keeps editor name, remains preview until Save. | `reset-actions-running-check.json`; full reset regression |
| Save and apply / rename | Saves named palette and active identity; duplicate rename rejected. All 38 values and identity survive exact-package process restart. | `import-export-copy-running-check.json`, `package-import-save-restart-running-check.json` |
| Save a copy | Unique new name; original and active identity preserved; Cancel restores original. | `import-export-copy-running-check.json`; disk persistence regression |
| Cancel / close / return to editor | Cancel and close discard draft without changing saved configuration; return retains draft; competing Settings mode/preset actions are disabled during editing. Exact-package edit/reset/Cancel also passes. | `draft-protection-running-check.json`, `package-edit-reset-cancel-running-check.json` |
| Import / export | v1 envelope round-trips all 38 values and metadata; import stays preview until saved; unique imported identity preserves existing records. Real owned GTK chooser exercised in development and exact package. | `native-theme-chooser-running-check.json`, `import-export-copy-running-check.json`, `package-import-save-restart-running-check.json` |
| Saved Apply / Delete | Apply changes rendering and disk; inactive delete preserves active record; active delete returns to that record's base mode. Both opposite-mode directions survive process restart. | `saved-theme-actions-running-check.json`, `opposite-mode-delete-restart-running-check.json` |

## Windows, feedback and layout

Shared viewport frames reach main, Settings, editor and Inspector. Exact-package native 150% checks with window radius 20 and border width 4 verify all four transparent native corners and matching border/panel/input colors after restart. Direct X11 pixel alpha is used because ordinary screenshots flatten transparency. Package executable hash matches the release build and the artifact hash matches clean-source provenance.

Ottrin-owned Rename, Create Folder, Delete, About, Move Conflict, Empty Trash and Open With dialogs, Details sidebar, main/context/More menus, color popover and tooltip were rendered and inspected. Distinct panel, raised/input, heading, button and frame roles match their intended consumers. List/Grid/Column canvases share exact content color; cancellation preserves conflict files and private trash. Evidence: `dialog-roles-running-check.json`, `additional-consumers-running-check.json`, `shared-native-window-roles-running-check.json`.

Filled buttons retain their background beneath hover/press overlays; normal menu labels use Button text, destructive labels use Error & danger. Inputs use raised fill/surface corners and Accent focus outline independently of primary/selected text ink. Rendered regressions and fresh live pixels verify these roles (`live-widget-role-refinements-running-check.json`). Color rows have aligned label/swatch/hex/reset columns; short numeric and color sliders replace oversized controls. Text scale 0.85–1.35 has its own typography group; maximum-scale development checks and final native display-scale captures verify usable layout.

All seven preset palettes in both modes pass 8,708 readability checks: interface accent/text combinations and every semantic code subtype in five file-row states, with both label-color flags. User-selected colors remain exact and unrestricted. Final source checks: 325 UI tests, 32 core tests, strict UI library/test Clippy, formatting/whitespace and package lifecycle contract pass.

## Verified platform boundaries

The OS file chooser belongs to the desktop portal and follows the desktop theme; Ottrin's theme applies to Ottrin-owned windows. Chooser PID/executable and private profile ownership were verified before interaction. Native Linux package checks used isolated X11 at 100%/150%; host desktop/preferences were not modified.

The Windows graphics compatibility notice is gated by Windows startup and cannot open in a Linux AppImage. Its real dialog renderer is covered by the shared five-dialog regression. Native Windows compositor behavior was not tested here; this is a Linux AppImage acceptance, not a Windows release sign-off.

## Delivery and cleanup

The stable AppImage was built from clean committed source; its provenance sidecar and checksum identify the artifact above. Final package evidence is kept with compact control/action receipts and rendered acceptance captures. Private Ottrin/index processes, displays, buses, GTK portals and profiles were stopped and removed; packaging checkout/link staging and superseded development theme captures were removed. Unrelated Preview Windows changes and user `AGENTS.md` remain untouched.
