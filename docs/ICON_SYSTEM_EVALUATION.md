# Icon System Evaluation

Status: design recommendation only; no icon-family migration is accepted by this document.

## Current baseline

Ottrin bundles `crates/ottrin-ui/assets/MaterialSymbolsFilled.ttf` and maps semantic file/device roles to Material Symbols codepoints through `icons.rs` and `semantic_style.rs`. This is operationally simple in egui: one bundled fallback font covers the existing interface and keeps tinting, scaling, and packaging uniform.

The current Locations problem is primarily presentation, not glyph availability: collapsed tiles rendered a semantic icon plus a redundant letter, slash, or drive cue, while generic folder symbols hid familiar Home/Desktop/Downloads/Documents distinctions. The V1 correction is an icon-only 44 px rail with distinct semantic Material symbols, a panel-specific expand/collapse affordance, full hover/focus details, and no layout gutter before the file view. Replacing the icon family would not by itself fix those proportions or semantics.

## Tabler Icons

Tabler publishes more than 6,100 MIT-licensed SVG icons, designed on a 24×24 grid with adjustable stroke width. Its device set is broad enough to prototype internal disks, USB/removable storage, optical media, network shares, folders, documents, archives, and common file categories.

**Advantages**

- Cohesive outline language and predictable 24×24 geometry.
- MIT license and individual SVG assets make a deliberately selected subset practical.
- Stroke width can be tuned for Ottrin's actual 16/20/24 px rendering sizes.
- SVG paths avoid private-use codepoint coupling and can be reviewed asset by asset.

**Costs and risks**

- Ottrin currently has a font-codepoint pipeline; SVG rendering/caching and theme tinting need a small measured prototype before migration.
- Outline icons may lose clarity against filled Material symbols at 16 px unless stroke and pixel alignment are tuned.
- A full replacement would touch a large visual surface and invalidate current packaged visual evidence.

Recommendation: prototype a **small selected Tabler SVG set**, not the whole library: folder, file, internal disk, USB/removable, optical, network share, unavailable, Windows/system volume, and Linux/system volume. Compare against current Material symbols at 16, 20, and 24 px in every bundled light/dark theme before choosing a migration.

Source: [Tabler Icons repository](https://github.com/tabler/tabler-icons) (MIT; 24×24 SVG family).

## Nerd Fonts

Nerd Fonts is an aggregator and patcher that combines glyphs from several upstream icon families into patched developer fonts. It offers broad filename and technology-logo coverage, but it is a weaker primary UI contract for Ottrin.

**Concerns**

- Glyphs come from multiple visual systems, so stroke, fill, baseline, and optical size are inconsistent.
- Private-use codepoints and font patch versions create a larger mapping/versioning surface than selected SVG assets.
- Licensing must be checked for the selected base font and each included upstream glyph set; the aggregator is not one simple visual-asset license decision.
- Font logos and brand marks can imply ownership or platform semantics that the filesystem metadata does not establish.
- A large symbol font increases packaging and QA scope merely to gain icons that Ottrin may never display.

Recommendation: do **not** adopt Nerd Fonts as Ottrin's primary icon system. A very small, separately licensed brand/logo asset may be considered later when product semantics genuinely require it, but not through a complete patched font.

Source: [Nerd Fonts repository](https://github.com/ryanoasis/nerd-fonts) (aggregator/patcher with multiple upstream collections and patched base fonts).

## Filesystems and operating-system logos

A filesystem type and an operating system are different facts. An NTFS volume may be shared storage and does not prove that Windows is installed there. Likewise, ext4 or Btrfs does not by itself identify a Linux system volume.

V1 should therefore use:

1. a semantic device icon (internal, removable, optical, network, unavailable);
2. the full location identity and mount path in hover/focus accessibility detail and expanded secondary text, never as a collapsed-rail letter, slash, drive label, or badge;
3. an optional textual filesystem badge (`NTFS`, `ext4`, `Btrfs`) only when discovery reports it reliably;
4. an operating-system logo only when a later, explicit system-volume detector has stronger evidence than filesystem type.

Identity and state must remain available as text/accessible detail and must never depend only on icon shape or color.

## Decision gate

Do not migrate until a focused native prototype proves all of the following:

- selected icons remain legible and aligned at 16/20/24 px;
- light, dark, high-contrast, selected, disabled, and unavailable states remain clear;
- egui SVG caching does not regress scrolling or startup;
- Linux portable/AppImage and Windows portable/installer contain the exact pinned assets and notices;
- semantic coverage is at least equal to the current `IconKey` mapping;
- manual side-by-side review prefers the replacement as a coherent system, not merely for a few attractive glyphs.
