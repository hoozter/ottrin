#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$ROOT/packaging/linux/bundle-runtime-libs.sh"
source "$ROOT/packaging/package-lifecycle.sh"
VERSION="${OTTRIN_VERSION:-$(grep -m1 '^version = ' "$ROOT/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')}"
DIST_DIR="${OTTRIN_DIST_DIR:-$ROOT/dist/packages}"
BUILD_DIR="$ROOT/target/release"
mkdir -p "$DIST_DIR"
WORK="$(mktemp -d "$DIST_DIR/.appimage.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
APPDIR="$WORK/AppDir"
APPIMAGE="$DIST_DIR/Ottrin-$VERSION-x86_64.AppImage"
APPIMAGETOOL="${APPIMAGETOOL:-$ROOT/dist/appimagetool-x86_64.AppImage}"
APPIMAGETOOL_SHA256="${APPIMAGETOOL_SHA256:-b90f4a8b18967545fda78a445b27680a1642f1ef9488ced28b65398f2be7add2}"
PDFIUM_URL="https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/7961/pdfium-linux-x64.tgz"
PDFIUM_SHA256="019665c8877d46fe65f625f80fd714ab07aac68554b0636acf2a2adf9288adb2"


download_verify_and_package_pdfium() (
    local target_pdfium_dir="$1"
    local tmp_dir
    tmp_dir="$(mktemp -d)"
    trap 'rm -rf "$tmp_dir"' EXIT
    local archive="$tmp_dir/pdfium.tgz"
    local extract_dir="$tmp_dir/extract"
    mkdir -p "$extract_dir"

    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$PDFIUM_URL" -o "$archive"
    else
        wget -q "$PDFIUM_URL" -O "$archive"
    fi
    printf '%s  %s\n' "$PDFIUM_SHA256" "$archive" | sha256sum -c -
    tar -xzf "$archive" -C "$extract_dir"

    mkdir -p "$target_pdfium_dir"
    mkdir -p "$target_pdfium_dir/lib"
    cp -a "$extract_dir/lib/." "$target_pdfium_dir/lib/"
    cp -a "$extract_dir/LICENSE" "$extract_dir/licenses" "$target_pdfium_dir/"
)

cd "$ROOT"
if [[ "${OTTRIN_SKIP_BUILD:-0}" != "1" ]]; then
    cargo build --release -p ottrin-app -p ottrin-platform -p ottrin-search
    cargo build --release -p ottrin-preview --bin ottrin-media-playback --features system-gstreamer
    cargo build --release -p ottrin-preview --bin ottrin-legacy-doc-render
fi

rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin"
mkdir -p "$APPDIR/usr/libexec/ottrin"
mkdir -p "$APPDIR/usr/share/applications"
mkdir -p "$APPDIR/usr/share/icons/hicolor/scalable/apps"
mkdir -p "$APPDIR/usr/share/icons/hicolor/256x256/apps"
mkdir -p "$APPDIR/usr/share/metainfo"
mkdir -p "$APPDIR/usr/share/ottrin"
mkdir -p "$APPDIR/usr/share/polkit-1/actions"
mkdir -p "$APPDIR/usr/share/doc/ottrin"

install -m 0755 "$ROOT/target/release/ottrin-app" "$APPDIR/usr/bin/ottrin"
install -m 0755 "$ROOT/target/release/ottrin-legacy-doc-render" "$APPDIR/usr/bin/ottrin-legacy-doc-render"
install -m 0755 "$ROOT/target/release/ottrin-media-playback" "$APPDIR/usr/bin/ottrin-media-playback"
install -m 0755 "$ROOT/target/release/ottrin-indexd" "$APPDIR/usr/bin/ottrin-indexd"
install -m 0755 "$ROOT/target/release/ottrin-priv-helper" "$APPDIR/usr/libexec/ottrin/ottrin-priv-helper"
download_verify_and_package_pdfium "$APPDIR/usr/bin/pdfium"
install -m 0644 "$ROOT/assets/icon.svg" "$APPDIR/usr/share/icons/hicolor/scalable/apps/ottrin.svg"
install -m 0644 "$ROOT/assets/icon.png" "$APPDIR/usr/share/icons/hicolor/256x256/apps/ottrin.png"
install -m 0644 "$ROOT/packaging/linux/ottrin.appdata.xml" "$APPDIR/usr/share/metainfo/org.ottrin.Ottrin.appdata.xml"
install -m 0644 "$ROOT/packaging/linux/org.ottrin.filesystem.policy" "$APPDIR/usr/share/polkit-1/actions/"
install -m 0644 "$ROOT/packaging/linux/ottrin-index.service.in" "$APPDIR/usr/share/ottrin/"
install -m 0644 "$ROOT/README.md" "$ROOT/LICENSE" "$APPDIR/usr/share/doc/ottrin/"
install -m 0644 "$ROOT/docs/USER_GUIDE.md" "$ROOT/docs/DEVELOPMENT.md" \
  "$ROOT/docs/ROADMAP.md" "$ROOT/docs/RELEASE_NOTES.md" \
  "$ROOT/docs/RELEASE_CHECKLIST.md" "$ROOT/docs/SEARCH_PERFORMANCE.md" \
  "$APPDIR/usr/share/doc/ottrin/"
bundle_linux_runtime "$BUILD_DIR/ottrin-app" "$APPDIR/usr" \
  "$APPDIR/usr/share/doc/ottrin/third-party-notices/linux-runtime" \
  "$BUILD_DIR/ottrin-legacy-doc-render"
ottrin_write_source_revision "$ROOT" "$APPDIR/usr/share/doc/ottrin/SOURCE_REVISION.txt" "$VERSION"

cat > "$APPDIR/org.ottrin.Ottrin.desktop" <<'DESKTOP'
[Desktop Entry]
Name=Ottrin
Comment=Keyboard-first file manager
Exec=ottrin
Icon=ottrin
Type=Application
Categories=System;FileTools;FileManager;
Keywords=files;manager;miller;
DESKTOP
cp "$APPDIR/org.ottrin.Ottrin.desktop" "$APPDIR/usr/share/applications/org.ottrin.Ottrin.desktop"

cat > "$APPDIR/AppRun" <<'APPRUN'
#!/bin/sh
APPDIR="$(dirname "$(readlink -f "$0")")"
export LD_LIBRARY_PATH="$APPDIR/usr/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export OTTRIN_PRIV_HELPER=/usr/libexec/ottrin/ottrin-priv-helper
case "${1:-}" in
  --index-daemon|--administrator-index-daemon)
    exec "$APPDIR/usr/bin/ottrin-indexd" "$@"
    ;;
esac
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

printf '%s  %s\n' "$APPIMAGETOOL_SHA256" "$APPIMAGETOOL" | sha256sum -c -

ARCH=x86_64 "$APPIMAGETOOL" "$APPDIR" "$WORK/$(basename "$APPIMAGE")"
ottrin_publish_artifact "$WORK/$(basename "$APPIMAGE")" "$APPIMAGE"
sha256sum "$APPIMAGE" | sed "s|$APPIMAGE|$(basename "$APPIMAGE")|" > "$APPIMAGE.sha256"
artifact_sha256="$(sha256sum "$APPIMAGE" | awk '{print $1}')"
revision="$(git -C "$ROOT" rev-parse --verify HEAD)"
if git -C "$ROOT" diff --quiet --ignore-submodules -- && \
   git -C "$ROOT" diff --cached --quiet --ignore-submodules --; then
  clean_source=true
else
  clean_source=false
fi
cat > "$APPIMAGE.provenance.json" <<EOF
{
  "artifact": "$(basename "$APPIMAGE")",
  "revision": "$revision",
  "sha256": "$artifact_sha256",
  "built_from_tracked_clean_source": $clean_source
}
EOF

printf '%s\n' "$APPIMAGE"
