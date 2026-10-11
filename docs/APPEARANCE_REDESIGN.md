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

Implementation and acceptance are in progress; no completion is claimed by this document.

## Independent variant rendering

Custom colors and geometry now resolve from the selected stored variant without consulting the source preset. File-label neutral blend strength was previously a preset-name lookup; it is now stored and editable per variant, preserving the source theme’s label coloring while removing that last rendering dependency. The 38 existing fields remain available, with this one additional value in File labels & icons. Source preset metadata describes where a theme was created.

## Development verification

Focused interaction tests verify independent card body/pencil hit targets, variant switching without changing the global preference, paired persistence, cancellation, and retaining a dirty session when its window closes. Native development screenshots identified an unsupported pencil glyph and a clipped reset button at 800 × 600; fixes use the bundled Material edit icon and budget the actual widths of color inputs and reset targets. These observations are development evidence; the final artifact has not yet been rebuilt or accepted for this redesign.


The current development source passes 329 UI tests, 32 core tests, strict UI/core Clippy, formatting, and diff validation. The preset-copy regression compares the entire rendered Colors value for all seven presets in both modes. The minimum-layout regression paints all 11 inspector contexts at 800 × 600 with text scales 1.0 and 1.35 and checks aligned, visible reset targets.

Native Linux testing covered built-in apply/customize/cancel/create, independent variants, existing edit/save, thumbnail updates, Save as new, rename/delete with active fallback, global modes, dirty close/continue, and explicit app-exit choices. Additional native checks verified individual-property reset, theme-default reset preserving a button-radius edit, whole-theme reset restoring both original palettes, and v2 export/import preserving the complete paired configuration and global preference. Numeric serialization is compared at native f32 precision.

At 150% desktop scaling, the 800 × 600 logical editor remains functional in both modes. Increasing the Dark variant's text size to 1.35 preserves aligned color/numeric/reset columns, wraps long labels, keeps the preview visible during independent inspector scrolling, and retains accessible color/opacity controls. Native GUI evidence is in `.hermes/evidence/appearance-redesign/`; it is development evidence until exact AppImage verification is recorded.

The v2 import/export envelope stores `format: ottrin-theme`, `version: 2`, and one named theme with complete `light` and `dark` palettes. Built-in provenance is descriptive only. There is no single-variant v1 compatibility path. Color overlays and semantic label blending derive from the stored palette, so changing the descriptive source preset cannot change a saved theme.
