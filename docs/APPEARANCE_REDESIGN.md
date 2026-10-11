# Appearance redesign

## Delivery contract

Implement the attached Appearance Settings and Theme Editor Redesign brief in full. Built-in presets are immutable; explicit Create saves a new independent custom theme with complete Light and Dark variants. Edit updates an existing custom theme; Save as new duplicates it. Global Light/Dark/System preference remains independent of the editor variant. Preserve all 38 customization fields, import/export, keyboard access, resets, and predictable cancellation and unfinished sessions.

Use one data-driven miniature renderer for both theme libraries. Restore separate card pencil actions without selecting the theme. Both libraries use equal card dimensions and effective-mode thumbnails. Use a connected global mode control and a compact active-theme summary, with contextual Continue editing when needed.

Editor: compact ownership-aware header and naming; approximately 58% persistent interactive preview, 42% independently scrolling inspector. A compact component selector replaces the permanent button collection. Theme defaults are a separate context; each component only exposes relevant colors, typography, geometry, interaction states, and file-category properties. No nested card maze or redundant theme navigation. Maintain themed styling, consistent control rows, focus, hover, and responsive layout.

Closing a dirty editor retains the unfinished session; explicit Cancel discards it. Theme selection cannot conflict with a live draft. App exit must not silently discard a dirty draft. Reset property, component, and whole-theme actions clearly identify their scopes.

## Assessment before implementation

Built-in miniature previews are painter-generated from Colors, with some fixed shape details. Custom library entries currently use only a five-color strip. The live editor preview is separately painted and clickable. SavedTheme currently stores one complete customization, a base preset, and a fixed mode; selecting a custom theme disables global mode selection. Built-in edits already save new records, but both variants are not retained; switching mode regenerates the palette and loses unsaved changes. Closing the editor currently cancels immediately. The editor mixes selected component colors with global defaults and has five competing categories plus a duplicate component button collection.

The personal configuration contained zero saved custom themes at assessment. Replace the single-palette saved model and v1 export with paired complete palettes and a v2 export; update callers and tests instead of retaining obsolete storage paths. Source preset metadata is descriptive; resets restore the captured values from the start of the editing session, and runtime custom rendering must derive represented styling from the stored palette.

## Acceptance evidence required

Verify all 14 workflows from the brief: built-in apply; pencil without activation; cancel without creation; create/activate; immutable original; accurate custom preview; edit/save; changed preview; Save as new; rename/delete and valid active reference; three global modes; independent variants; cancel restoration; all controls accessible. Also verify dirty close/continue and exit handling, reset scopes, import/export, identical previews across ownership, small desktop layout, 100%/150% scaling, Light/Dark rendering, alignment, clipping, focus, and independent inspector scrolling. Bind final packaged GUI evidence to the exact stable AppImage revision and checksum. Preserve unrelated preview/Windows work and personal settings/index data; remove owned test processes, profiles, staging, and obsolete generated output.

Implementation and Linux AppImage acceptance are complete. The evidence below binds the final deliverable to its exact source revision and checksum.

## Independent variant rendering

Custom colors and geometry now resolve from the selected stored variant without consulting the source preset. File-label neutral blend strength was previously a preset-name lookup; it is now stored and editable per variant, preserving the source theme’s label coloring while removing that last rendering dependency. The 38 existing fields remain available, with this one additional value in File labels & icons. Source preset metadata describes where a theme was created.

## Original redesign delivery — 2026-10-11

The original accepted redesign used `dist/packages/Ottrin-0.1.0-x86_64.AppImage`.
It was built from clean tracked revision `a3a76ac1d26a88c3cde026faaf74d934e25faad5`;
SHA-256: `2aca0c0376dd6f99e235fbb386e79982b856c15fbe2ba58d99cb062d070ae8ac`.
The original acceptance receipt is retained in `.hermes/evidence/appearance-redesign/`. The current package and its provenance are described in the corrections section below.

Source verification passed **331 UI tests**, **32 core tests**, **18 focused appearance tests**, strict UI/core Clippy, formatting, and diff validation. Tests exercise card click-target separation, paired persistence, save/cancel/rename/copy/delete, imported-draft ownership, all inspector controls and reset alignment at 800 × 600 with text scales 1.0 and 1.35, semantic component isolation, dialog styling, complete preset palette equality for seven presets in both modes, dynamic miniature button/accent rendering, and filename elision.

Native verification ran the final AppImage on private Linux X11 displays and private profiles, at 100% and 150% desktop scaling. Every one of the **33 color and 6 numeric controls** was edited through the interface, reset, edited again, saved, and checked after restarting the same artifact. All 39 property resets restored the preview pixel-for-pixel. Complete Light/Dark values and the independent global preference were preserved. Changes were inspected in the preview and real file-manager UI; heading text was additionally checked in the editor heading, and audio/font category colors in real sample files. The original 38 settings remain accessible; neutral file-label blend is the additional control.

### Full brief audit

| Brief section | Delivered behavior and authoritative evidence |
|---|---|
| 1. General design | Shared themed controls and semantic roles; compact header/footer, flat contextual groups, consistent input/reset columns. Native 100% and 150% Light/Dark screenshots; rendered alignment and component-isolation tests. |
| 2. Appearance library | Equal-size dynamic cards, independent subtle pencil targets and ownership tooltips, selected/focus indicators, connected mutually exclusive Light/Dark/System control and effective-mode label, concise active theme and contextual Continue editing. Native apply/pencil/retained-draft and keyboard System activation; card-target regression. |
| 3. Ownership and variants | Immutable built-ins; explicit Create only; complete independent paired custom palettes; automatic Customize/Edit titles/actions; in-place Save, rename, Save as new, duplicate-name handling, custom-only delete and valid fallback. Native management snapshots and source lifecycle tests. |
| 4. Preview architecture | One lightweight painter-based miniature for both ownership types, fed by effective palette; no PNG dependency or preset-name styling lookup. Actual backgrounds, surfaces, selection, text, borders, accent, button fill/text/corners. Native updated-thumbnail and immutable-original captures; complete Colors equality and miniature regressions. |
| 5. Editor workspace | Approximately 58% persistent clickable preview / 42% contextual inspector; one synchronized component selector for all 11 contexts; Defaults separate from component colors/geometry/categories; independent inspector scrolling; header naming and ownership, secondary variant control, unsaved state, stable action footer. Native minimum-size, selection, scroll and control sweep; rendered layout/isolation tests. |
| 6. Lifecycle and resets | Live draft without persistence, explicit Save/Create/Cancel, dirty close retains a resumable draft, conflicting library actions disabled, exit Save/Discard/Keep choices. Reset property/context/current variant versus both starting palettes. Native exact-record comparisons, close/continue and all exit choices. |
| 7. Visual consistency | Shared theme buttons, fields, segmented controls, typography and surface/window roles; both modes verified. Reuses existing tokens and icon font. Native rendered screens and dialog/semantic role tests. |
| 8. Preserved capabilities | All seven built-ins, all existing fields plus blend, global modes independent of paired editor variants, typography/geometry/category/state editing, persistence and v2 paired export/import. No obsolete v1 or single-palette compatibility path. Control sweep, full-palette tests and native import/export records. |
| 9–10. Delivery and integrated design | Pre-implementation assessment retained above; implementation committed, source checks passed, actual final-package GUI inspected, guide updated, compact acceptance receipts published, task-owned processes/profiles/staging removed. |

The architecture stores complete values rather than inheritance/override flags. Theme defaults are shared tokens, and component contexts expose their own relevant stored fields. Reset tooltips explicitly restore the values captured when the editor opened; component reset affects only that context in the current variant. More → Reset theme to starting values restores both palettes. No duplicate inheritance state was introduced.

### Required native workflows

| # | Workflow | Final-package evidence in `.hermes/evidence/appearance-redesign/` |
|---:|---|---|
| 1 | Apply built-in | `final-workflow-builtin-applied.json` and screenshot |
| 2 | Pencil without activation | `final-workflow-pencil-without-activation.json` exactly equals applied state |
| 3 | Cancel without creation | `final-workflow-cancel-no-create.json` exactly equals applied state |
| 4 | Create and activate | `final-workflow-created.json` |
| 5 | Immutable original | `final-workflow-immutable-nord.png`; original thumbnail pixel-identical |
| 6 | Correct custom thumbnail | `final-workflow-custom-applied.png` |
| 7 | Edit existing and save | `final-workflow-edited.json` |
| 8 | Updated preview | `final-workflow-updated-thumbnail.png` |
| 9 | Save as new | `final-workflow-duplicated.json`; copy has identical independent paired palettes |
| 10 | Rename/delete and valid active reference | `final-workflow-renamed.json`, `final-workflow-deleted-active.json` |
| 11 | Light/Dark/System selection and previews | `final-workflow-mode-*.json/png`, `final-keyboard-system-selected.json/png` |
| 12 | Independent variants | Created Dark panel `#203040`, Light panel `#DCE6F0`; `final-workflow-independent-variants.png` |
| 13 | Cancel restores prior appearance | `final-workflow-cancel-restored.json` and native cancel captures |
| 14 | Every supported control accessible and effective | `FINAL_CONTROL_ACCEPTANCE.json`, saved/restarted complete records and representative control captures |

Additional exact-package receipts:

- `FINAL_PICKER_IMPORT_ACCEPTANCE.json`: hue/saturation/brightness, numeric RGB/opacity, heading edit/reset, keyboard mode activation and native v2 export/import. RGB/opacity persisted `[80,120,160,89]`; import remained unsaved until Create.
- `FINAL_SCALE_EXIT_ACCEPTANCE.json`: 800 × 600 logical editor at 150%, text 1.35×, both modes, independent scroll, long-label wrapping/reset alignment, filename ellipsis, compact picker, Keep editing, Save and close, exact restart persistence.
- `FINAL_RESET_EXIT_ACCEPTANCE.json`: context reset preserves other context/variant, whole reset restores both palettes exactly, Discard and close preserves the saved paired record and global preference across restart.
- `ACCEPTANCE.json` and `dist/packages/APPEARANCE_ACCEPTANCE.json`: compact final delivery inventory and artifact binding.

### Model and boundaries

The v2 export envelope stores `format: ottrin-theme`, `version: 2`, and one named theme with complete `light` and `dark` palettes. Source preset metadata is descriptive only. Color overlays and semantic label blending derive from the stored palette; changing descriptive provenance cannot change a saved theme.

These are Linux X11 native-package results. OS-owned portal file choosers follow the desktop theme, and physical Windows compositor behavior is not certified by this Linux run. Private QA disabled the unavailable private accessibility bridge; this is not screen-reader acceptance. Keyboard operation and focus behavior were tested directly. Personal settings and index data were preserved, unrelated Windows preview work remains untouched, and the installed index service was verified separately against the same package.


## Target and shared-style corrections — 2026-10-11

The accepted appearance correction replaced the original package above. It is built from clean tracked revision `30a3084966832587198cfcfa9a5d3283c2502ca5`, with SHA-256 `765958cca21391d2ab01e8c962f6ea4ebb69b930f8cc9cf89a4ff55d3446804d`. Its adjacent provenance and `APPEARANCE_ACCEPTANCE.json` bind the exact deliverable to these checks.

Selected-row painting, hit testing and editing outlines now use one rectangle, eliminating the ten-pixel offset. File-canvas and row-fill outlines remain square. Tabs and navigation are one selectable context; both are highlighted, and actual tabs and the preview share their fill blend, muted text and top-corner geometry. Relevant shared colors, typography and geometry are available beside each component, with visible ownership and reset explanations. Window corners affect application windows, surface corners affect tabs/inputs/expanded location cards, and button corners affect buttons/collapsed location icons. Hex fields scale with text size.

That correction’s source verification: **333 UI tests**, including **20 focused appearance tests**, strict UI Clippy, formatting and diff checks. Focused native verification ran this exact AppImage at 100% and 150%, including an 800 × 600 logical editor and 1.35 text scale. It checked aligned row and square canvas targets, tab selection, real tab/toolbar color and radius changes, shared-reset isolation, saved paired values, Light/Dark controls and readable hex fields. Evidence is in `.hermes/evidence/theme-targets/`. The original exhaustive 39-control/lifecycle sweep above remains historical evidence for the original redesign package; it was not repeated in full for this correction.

The unchanged media helper was reused only after its checksum matched the prior accepted package; rebuilding it was blocked by unavailable host GStreamer development files. Personal configuration checksum is unchanged, personal index data and unrelated Windows preview work were preserved. Owned private QA processes, profiles and packaging worktree were removed. Later documentation-only commits do not change the packaged code.


## Settings and preview coherence correction — 2026-10-11

The latest stable AppImage is built from clean tracked revision `83f5ce607651c59b926632a91325990dca28fcd9`, SHA-256 `abc41cdfc6b75a05724e113fc002b7ec97019c4eeaba4d12b3fa5a56299a5804`. Exact-package evidence is in `.hermes/evidence/theme-coherence/ACCEPTANCE.json` and adjacent `dist/packages/APPEARANCE_ACCEPTANCE.json`.

Settings navigation and editor property groups now share one framed theme surface. Navigation rows allocate bounded rectangles and centre their icons/text consistently. The editor uses the same edge-to-edge header and line-based Close control as the other native windows. Property groups use explicit larger headings, borders/dividers and compact spacing; input surfaces remain visually distinct. Component colors, shared colors, typography and geometry are separate groups. The former Sidebar target is named Smart Panel and exposes its actual shared contrast and button-corner controls.

The preview and production Smart Panel share the icon-strip renderer, icon order, hover/active styling, toggle behavior and panel-header renderer. Info stays inline in the Miller fixture; other sections expand/collapse locally. The preview still uses sample files and does not execute real search or file operations. This correction removes fabricated text buttons; it does not claim every preview surface is a complete live file-manager instance.

Source checks passed 335 UI tests at `217b4a4`, then 22 focused appearance tests after the hierarchy/label correction at `3c5b91a`; strict all-target UI Clippy, formatting and diff checks pass at `83f5ce6`. Final-package native checks cover Settings/editor alignment and framing, Light/Dark at 100%/150%, Smart Panel picking/toggling, scrolling, button radius 14, text size 1.35 at 150%, and Cancel restoration. A live titlebar color change produced identical RGB [113,58,82] in the editor, preview and Settings. The historical full 39-control persistence/reset matrix remains separate; it was not repeated for these layout changes. Native Windows acceptance remains pending.

Personal preferences retain their original checksum; personal index/service and unrelated Windows work are preserved. The unchanged media helper was checksum-verified and reused. Owned QA processes, private profiles, extracted runtimes and packaging staging were removed; shared compiler caches remain in use.

## Interface alignment audit — 2026-10-11

Menu and repeated choice rows now reserve independent indicator, icon and label columns, including unchecked, iconless and disabled rows. View Options, the main menu, file/destination submenus, Inspector/tab menus, editor More actions, Settings choices and Drop Folder rows use the shared renderer. Native Button retains interaction, focus and label semantics. Existing structured file rows, column checkbox menus, Settings navigation, property rows and dialog choices were reviewed rather than replaced with another implementation.

Enlarged-text QA also exposed compact left-rail icon overflow and clipped Drop Folder trailing actions. Compact padding, explicit trailing-space reservation and bundled Material up/down glyphs correct those cases. The selected destination retains its label start; narrow-row and mixed-state rendered geometry regressions cover bounds, overlap and centering.

Theme consistency and text/icon alignment are standing requirements in `docs/DEVELOPMENT.md`, with an explicit UI coherence gate in `docs/RELEASE_CHECKLIST.md`.

The AppImage accepted for this alignment audit was built from clean tracked revision `436924961e4f0d964bebf85e844919007f1ced09`; SHA-256 `2d65285a38bc4b94cc6a5e037958e49bc43bd09eb88480629eb8adec3a345ae9`. All 337 UI library tests, strict UI all-target Clippy, formatting and diff checks pass. Exact-package native checks cover Ottrin Dark/Light at 100%, Ottrin Light at 150%, and a Nord custom fixture at 150% with 1.35× text. Main/View Options/Settings were checked in all four configurations; file, folder submenu, Inspector, tab and editor More menus were checked at 100%. Drop Folder selection, arrow glyphs and action bounds were verified at enlarged text. This focused acceptance does not repeat the full theme property/persistence sweep or native Windows acceptance.

Receipt: `.hermes/evidence/menu-alignment/ACCEPTANCE.json`, also published beside the AppImage as `APPEARANCE_ACCEPTANCE.json`. Personal configuration checksum is unchanged, personal index/service and unrelated Windows work were preserved. Owned test processes, profiles, extracted runtimes and packaging checkout were removed; useful shared target output remains.
