#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$ROOT/packaging/package-lifecycle.sh"
VERSION="${OTTRIN_VERSION:-$(grep -m1 '^version = ' "$ROOT/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')}"
DIST_DIR="${OTTRIN_DIST_DIR:-$ROOT/dist}"
ARCH="${OTTRIN_DEB_ARCH:-$(dpkg --print-architecture)}"
OUTPUT="$DIST_DIR/ottrin_${VERSION}_${ARCH}.deb"
mkdir -p "$DIST_DIR"
TMP="$(mktemp -d "$DIST_DIR/.deb.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
PACKAGE_ROOT="$TMP/package"

"$ROOT/packaging/linux/stage-native-package.sh" "$PACKAGE_ROOT"
# Native packages must reject systems older than the actual executable ABI.
# Inspect every executable: a helper can require a newer libc than the GUI.
GLIBC_MIN="$(
  objdump -T \
    "$PACKAGE_ROOT/usr/bin/ottrin" \
    "$PACKAGE_ROOT/usr/bin/ottrin-indexd" \
    "$PACKAGE_ROOT/usr/bin/ottrin-legacy-doc-render" \
    "$PACKAGE_ROOT/usr/bin/ottrin-media-playback" \
    "$PACKAGE_ROOT/usr/libexec/ottrin/ottrin-priv-helper" \
    | sed -nE '/\*UND\*/s/.*\(GLIBC_([0-9]+(\.[0-9]+)+)\).*/\1/p' \
    | sort -Vu | tail -n 1
)"
test -n "$GLIBC_MIN" || {
  printf 'Could not determine the native payload libc requirement.\n' >&2
  exit 1
}
mkdir -p "$PACKAGE_ROOT/DEBIAN" "$DIST_DIR"
cat > "$PACKAGE_ROOT/DEBIAN/control" <<EOF
Package: ottrin
Version: $VERSION
Section: utils
Priority: optional
Architecture: $ARCH
Maintainer: Ottrin <hello@hoozter.com>
Depends: libc6 (>= $GLIBC_MIN), libgcc-s1, libfontconfig1, libglib2.0-0, libdbus-1-3, libegl1, libgl1, libudev1, libwayland-client0, libx11-6, libxcb1, libxkbcommon0, libgstreamer1.0-0, libgstreamer-plugins-base1.0-0
Description: Fast, keyboard-friendly file manager
 Ottrin is a keyboard-first desktop file manager with Miller, List, and Grid views.
EOF

dpkg-deb --root-owner-group --build "$PACKAGE_ROOT" "$TMP/$(basename "$OUTPUT")"
ottrin_publish_artifact "$TMP/$(basename "$OUTPUT")" "$OUTPUT"
sha256sum "$OUTPUT" | sed "s|$OUTPUT|$(basename "$OUTPUT")|" > "$OUTPUT.sha256"
printf '%s\n' "$OUTPUT"
