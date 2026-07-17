#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION="${OTTRIN_VERSION:-$(grep -m1 '^version = ' "$ROOT/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')}"
DIST_DIR="$ROOT/dist"
APPDIR="$DIST_DIR/AppDir"
APPIMAGE="$DIST_DIR/Ottrin-$VERSION-x86_64.AppImage"
APPIMAGETOOL="${APPIMAGETOOL:-$DIST_DIR/appimagetool-x86_64.AppImage}"

cd "$ROOT"
cargo build --release -p ottrin-app -p ottrin-platform -p ottrin-search

rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin"
mkdir -p "$APPDIR/usr/libexec/ottrin"
mkdir -p "$APPDIR/usr/share/applications"
mkdir -p "$APPDIR/usr/share/icons/hicolor/scalable/apps"
mkdir -p "$APPDIR/usr/share/icons/hicolor/256x256/apps"
mkdir -p "$APPDIR/usr/share/polkit-1/actions"

install -m 0755 "$ROOT/target/release/ottrin-app" "$APPDIR/usr/bin/ottrin"
install -m 0755 "$ROOT/target/release/ottrin-index" "$APPDIR/usr/bin/ottrin-index"
install -m 0755 "$ROOT/target/release/ottrin-priv-helper" "$APPDIR/usr/libexec/ottrin/ottrin-priv-helper"
install -m 0644 "$ROOT/assets/icon.svg" "$APPDIR/usr/share/icons/hicolor/scalable/apps/ottrin.svg"
install -m 0644 "$ROOT/assets/icon.png" "$APPDIR/usr/share/icons/hicolor/256x256/apps/ottrin.png"
install -m 0644 "$ROOT/packaging/linux/org.ottrin.filesystem.policy" "$APPDIR/usr/share/polkit-1/actions/"

cat > "$APPDIR/ottrin.desktop" <<'DESKTOP'
[Desktop Entry]
Name=Ottrin
Comment=Keyboard-first file manager
Exec=ottrin
Icon=ottrin
Type=Application
Categories=System;FileManager;
Keywords=files;manager;miller;
DESKTOP
cp "$APPDIR/ottrin.desktop" "$APPDIR/usr/share/applications/ottrin.desktop"

cat > "$APPDIR/AppRun" <<'APPRUN'
#!/bin/sh
APPDIR="$(dirname "$(readlink -f "$0")")"
export OTTRIN_PRIV_HELPER="$APPDIR/usr/libexec/ottrin/ottrin-priv-helper"
if [ "$1" = "--ottrin-index" ]; then
  shift
  exec "$APPDIR/usr/bin/ottrin-index" "$@"
fi
exec "$APPDIR/usr/bin/ottrin" "$@"
APPRUN
chmod +x "$APPDIR/AppRun"

cp "$ROOT/assets/icon.svg" "$APPDIR/ottrin.svg"
cp "$ROOT/assets/icon.png" "$APPDIR/ottrin.png"

mkdir -p "$DIST_DIR"
if [ ! -x "$APPIMAGETOOL" ]; then
  wget -q "https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage" \
    -O "$APPIMAGETOOL"
  chmod +x "$APPIMAGETOOL"
fi

rm -f "$APPIMAGE"
ARCH=x86_64 "$APPIMAGETOOL" "$APPDIR" "$APPIMAGE"
sha256sum "$APPIMAGE" | sed "s|$APPIMAGE|$(basename "$APPIMAGE")|" > "$APPIMAGE.sha256"

printf '%s\n' "$APPIMAGE"
