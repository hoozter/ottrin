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
GLIBC_MIN="$(ottrin_native_glibc_requirement "$PACKAGE_ROOT")"
mkdir -p "$PACKAGE_ROOT/DEBIAN" "$DIST_DIR"
cat > "$PACKAGE_ROOT/DEBIAN/control" <<EOF
Package: ottrin
Version: $VERSION
Section: utils
Priority: optional
Architecture: $ARCH
Maintainer: Ottrin <hello@hoozter.com>
Depends: systemd, util-linux, libc6 (>= $GLIBC_MIN), pkexec, polkitd, libgcc-s1, libfontconfig1, libglib2.0-0, libdbus-1-3, libegl1, libgl1, libudev1, libwayland-client0, libx11-6, libx11-xcb1, libxcursor1, libxi6, libxcb1, libxkbcommon0, libxkbcommon-x11-0, libgstreamer1.0-0, libgstreamer-plugins-base1.0-0
Description: Fast, keyboard-friendly file manager
 Ottrin is a keyboard-first desktop file manager with Miller, List, and Grid views.
EOF

cat > "$PACKAGE_ROOT/DEBIAN/prerm" <<'SCRIPT'
#!/bin/sh
set -eu
if [ "$1" = remove ]; then
    /usr/libexec/ottrin/native-package-lifecycle remove
fi
SCRIPT
cat > "$PACKAGE_ROOT/DEBIAN/postinst" <<'SCRIPT'
#!/bin/sh
set -eu
if [ "$1" = configure ]; then
    /usr/libexec/ottrin/native-package-lifecycle refresh
fi
SCRIPT
chmod 0755 "$PACKAGE_ROOT/DEBIAN/prerm" "$PACKAGE_ROOT/DEBIAN/postinst"

dpkg-deb --root-owner-group --build "$PACKAGE_ROOT" "$TMP/$(basename "$OUTPUT")"
ottrin_publish_artifact "$TMP/$(basename "$OUTPUT")" "$OUTPUT"
sha256sum "$OUTPUT" | sed "s|$OUTPUT|$(basename "$OUTPUT")|" > "$OUTPUT.sha256"
printf '%s\n' "$OUTPUT"
