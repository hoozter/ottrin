#!/usr/bin/env bash
set -euo pipefail
umask 022

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$ROOT/packaging/linux/package-pdfium.sh"
source "$ROOT/packaging/package-search-runtime.sh"
VERSION="${OTTRIN_VERSION:-$(grep -m1 '^version = ' "$ROOT/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')}"
BUILD_DIR="${OTTRIN_BUILD_DIR:-$ROOT/target/release}"
DEST="${1:?usage: stage-native-package.sh DESTINATION}"

case "$DEST" in
  /|/usr|/usr/*)
    printf 'Refusing unsafe native package staging destination: %s\n' "$DEST" >&2
    exit 1
    ;;
esac

if [[ "${OTTRIN_SKIP_BUILD:-0}" != 1 ]]; then
  cd "$ROOT"
  cargo build --release -p ottrin-app -p ottrin-platform -p ottrin-search
  cargo build --release -p ottrin-preview --bin ottrin-media-playback --features system-gstreamer
  cargo build --release -p ottrin-preview --bin ottrin-legacy-doc-render --bin ottrin-html-render
fi

for executable in ottrin-app ottrin-indexd ottrin-priv-helper ottrin-media-playback ottrin-legacy-doc-render ottrin-html-render; do
  test -x "$BUILD_DIR/$executable" || {
    printf 'Missing native package executable: %s\n' "$BUILD_DIR/$executable" >&2
    exit 1
  }
done

rm -rf -- "$DEST"
mkdir -p \
  "$DEST/usr/bin" \
  "$DEST/usr/libexec/ottrin" \
  "$DEST/usr/share/applications" \
  "$DEST/usr/share/icons/hicolor/scalable/apps" \
  "$DEST/usr/share/icons/hicolor/256x256/apps" \
  "$DEST/usr/share/metainfo" \
  "$DEST/usr/share/ottrin" \
  "$DEST/usr/share/polkit-1/actions" \
  "$DEST/usr/share/doc/ottrin"

package_linux_pdfium "$DEST/usr/libexec/ottrin/pdfium"
package_search_runtime linux-x86_64 "$DEST/usr/libexec/ottrin" "$DEST/usr/share/doc/ottrin/third-party-notices/ripgrep"
install -D -m 0644 "$ROOT/crates/ottrin-ui/assets/THIRD_PARTY_NOTICES.md" "$DEST/usr/share/doc/ottrin/third-party-notices/BUNDLED_FONTS.md"

install -m 0755 "$BUILD_DIR/ottrin-app" "$DEST/usr/bin/ottrin"
install -m 0755 "$BUILD_DIR/ottrin-indexd" "$DEST/usr/bin/ottrin-indexd"
install -m 0755 "$BUILD_DIR/ottrin-legacy-doc-render" "$DEST/usr/bin/ottrin-legacy-doc-render"
install -m 0755 "$BUILD_DIR/ottrin-media-playback" "$DEST/usr/bin/ottrin-media-playback"
install -m 0755 "$BUILD_DIR/ottrin-priv-helper" "$DEST/usr/libexec/ottrin/ottrin-priv-helper"
install -m 0755 "$BUILD_DIR/ottrin-html-render" "$DEST/usr/libexec/ottrin/ottrin-html-render"
install -m 0644 "$ROOT/packaging/linux/ottrin-html-render.apparmor" "$DEST/usr/share/ottrin/"
install -m 0755 "$ROOT/packaging/linux/native-package-lifecycle.sh" "$DEST/usr/libexec/ottrin/native-package-lifecycle"
install -m 0644 "$ROOT/assets/icon.svg" "$DEST/usr/share/icons/hicolor/scalable/apps/ottrin.svg"
install -m 0644 "$ROOT/assets/icon.png" "$DEST/usr/share/icons/hicolor/256x256/apps/ottrin.png"
install -m 0644 "$ROOT/packaging/linux/ottrin.appdata.xml" "$DEST/usr/share/metainfo/org.ottrin.Ottrin.appdata.xml"
install -m 0644 "$ROOT/packaging/linux/org.ottrin.filesystem.policy" "$DEST/usr/share/polkit-1/actions/"
install -m 0644 "$ROOT/packaging/linux/ottrin-index.service.in" "$DEST/usr/share/ottrin/"
install -m 0644 "$ROOT/README.md" "$ROOT/LICENSE" "$DEST/usr/share/doc/ottrin/"
install -m 0644 \
  "$ROOT/docs/USER_GUIDE.md" \
  "$ROOT/docs/DEVELOPMENT.md" \
  "$ROOT/docs/ROADMAP.md" \
  "$ROOT/docs/RELEASE_NOTES.md" \
  "$ROOT/docs/RELEASE_CHECKLIST.md" \
  "$ROOT/docs/SEARCH_PERFORMANCE.md" \
  "$DEST/usr/share/doc/ottrin/"

cat > "$DEST/usr/share/applications/org.ottrin.Ottrin.desktop" <<'DESKTOP'
[Desktop Entry]
Name=Ottrin
Comment=Keyboard-first file manager
Exec=ottrin
Icon=ottrin
StartupWMClass=ottrin
Type=Application
Categories=System;FileTools;FileManager;
Keywords=files;manager;miller;
DESKTOP

SOURCE_REVISION="$(git -C "$ROOT" rev-parse --verify HEAD)"
TREE_STATE=clean
if ! git -C "$ROOT" diff --quiet --ignore-submodules -- \
  || ! git -C "$ROOT" diff --cached --quiet --ignore-submodules --; then
  TREE_STATE=dirty
fi
cat > "$DEST/usr/share/doc/ottrin/SOURCE_REVISION.txt" <<EOF
Ottrin source revision
Version: $VERSION
Git revision: $SOURCE_REVISION
Tracked tree: $TREE_STATE
EOF

printf '%s\n' "$DEST"
