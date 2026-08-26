#!/usr/bin/env bash
set -euo pipefail
umask 022

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION="${OTTRIN_VERSION:-$(grep -m1 '^version = ' "$ROOT/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')}"
BUILD_DIR="${OTTRIN_BUILD_DIR:-$ROOT/target/release}"
DEST="${1:?usage: stage-app-bundle.sh DESTINATION.app}"

case "$(basename -- "$DEST")" in
  *.app) ;;
  *)
    printf 'macOS app-bundle staging destination must end in .app: %s\n' "$DEST" >&2
    exit 1
    ;;
esac
case "$DEST" in
  /|/Applications|/Applications/*)
    printf 'Refusing unsafe macOS app-bundle staging destination: %s\n' "$DEST" >&2
    exit 1
    ;;
esac

if [[ "${OTTRIN_SKIP_BUILD:-0}" != 1 ]]; then
  if [[ "$(uname -s)" != Darwin ]]; then
    printf 'macOS app binaries must be built on macOS. Use this stager on a macOS host.\n' >&2
    exit 1
  fi
  cd "$ROOT"
  cargo build --release -p ottrin-app -p ottrin-search
fi

for executable in ottrin-app ottrin-indexd; do
  test -x "$BUILD_DIR/$executable" || {
    printf 'Missing macOS app-bundle executable: %s\n' "$BUILD_DIR/$executable" >&2
    exit 1
  }
done

rm -rf -- "$DEST"
mkdir -p "$DEST/Contents/MacOS" "$DEST/Contents/Resources/docs"
install -m 0755 "$BUILD_DIR/ottrin-app" "$DEST/Contents/MacOS/Ottrin"
install -m 0755 "$BUILD_DIR/ottrin-indexd" "$DEST/Contents/MacOS/ottrin-indexd"
install -m 0644 "$ROOT/assets/icon.png" "$DEST/Contents/Resources/Ottrin.png"
install -m 0644 "$ROOT/LICENSE" "$ROOT/README.md" "$DEST/Contents/Resources/"
install -m 0644 \
  "$ROOT/docs/USER_GUIDE.md" \
  "$ROOT/docs/DEVELOPMENT.md" \
  "$ROOT/docs/ROADMAP.md" \
  "$ROOT/docs/RELEASE_NOTES.md" \
  "$ROOT/docs/RELEASE_CHECKLIST.md" \
  "$DEST/Contents/Resources/docs/"

cat > "$DEST/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleDisplayName</key>
  <string>Ottrin</string>
  <key>CFBundleExecutable</key>
  <string>Ottrin</string>
  <key>CFBundleIconFile</key>
  <string>Ottrin.png</string>
  <key>CFBundleIdentifier</key>
  <string>org.ottrin.Ottrin</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>Ottrin</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>$VERSION</string>
  <key>CFBundleVersion</key>
  <string>$VERSION</string>
  <key>LSApplicationCategoryType</key>
  <string>public.app-category.utilities</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSPrincipalClass</key>
  <string>NSApplication</string>
</dict>
</plist>
EOF

SOURCE_REVISION="${OTTRIN_SOURCE_REVISION:-$(git -C "$ROOT" rev-parse --verify HEAD)}"
TREE_STATE=clean
if ! git -C "$ROOT" diff --quiet --ignore-submodules -- \
  || ! git -C "$ROOT" diff --cached --quiet --ignore-submodules --; then
  TREE_STATE=dirty
fi
cat > "$DEST/Contents/Resources/SOURCE_REVISION.txt" <<EOF
Ottrin source revision
Version: $VERSION
Git revision: $SOURCE_REVISION
Tracked tree: $TREE_STATE
EOF

printf '%s\n' "$DEST"
