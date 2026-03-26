# Theme Sources (Canonical)

Ottrin theme presets are mapped from the official upstream palettes/tokens below.

## Breeze (Dark/Light)
- Upstream: KDE Breeze color scheme files
- Source files:
  - `https://raw.githubusercontent.com/KDE/breeze/master/colors/BreezeDark.colors`
  - `https://raw.githubusercontent.com/KDE/breeze/master/colors/BreezeLight.colors`
- Key sections used: `[Colors:View]`, `[Colors:Window]`, `[Colors:Selection]`, `[Colors:Button]`

## Adwaita (Dark/Light)
- Upstream: GNOME libadwaita stylesheet and palette
- Source files:
  - `https://raw.githubusercontent.com/GNOME/libadwaita/main/src/stylesheet/_colors.scss`
  - `https://raw.githubusercontent.com/GNOME/libadwaita/main/src/stylesheet/_palette.scss`
- Key variables used:
  - `window_bg_color`, `window_fg_color`, `sidebar_bg_color`, `secondary_sidebar_bg_color`
  - `view_bg_color`, `view_fg_color`, `accent-blue`
  - dark-mode media-query overrides in `_colors.scss`

## Windows 11 (Dark/Light)
- Upstream: Microsoft Fluent UI design tokens
- Source files:
  - `https://raw.githubusercontent.com/microsoft/fluentui/master/packages/tokens/src/alias/lightColor.ts`
  - `https://raw.githubusercontent.com/microsoft/fluentui/master/packages/tokens/src/alias/darkColor.ts`
  - `https://raw.githubusercontent.com/microsoft/fluentui/master/packages/tokens/src/global/colors.ts`
- Key tokens used:
  - `colorNeutralBackground1/2/3/4`
  - `colorNeutralForeground1/2/3`
  - `colorNeutralStroke1`
  - `colorBrandBackground`, `colorBrandBackground2`, `colorBrandForeground1`
  - `red.primary` (for error)

## Solarized
- Upstream: Ethan Schoonover’s official Solarized palette
- Source file:
  - `https://raw.githubusercontent.com/altercation/solarized/master/README.md`
- Key colors used:
  - `base03..base3`, `blue`, `cyan`, `red`

## Nord
- Upstream: Nord official palette
- Source file:
  - `https://raw.githubusercontent.com/nordtheme/nord/develop/src/nord.css`
- Key colors used:
  - `--nord0` through `--nord15`
- Note:
  - Nord publishes a canonical palette, but not an official separate light theme definition.
  - Ottrin uses only official Nord color values; any light-mode mapping is palette-mapped, not an upstream \"Nord Light\" spec.

## G33k
- There is no canonical upstream "G33k" palette.
- This preset remains custom by design (terminal/green-screen inspired), and is intentionally not treated as an upstream-locked theme.
