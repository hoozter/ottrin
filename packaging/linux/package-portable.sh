#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION="${OTTRIN_VERSION:-$(grep -m1 '^version = ' "$ROOT/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')}"
BUILD_DIR="$ROOT/target/release"
DIST_DIR="${OTTRIN_DIST_DIR:-$ROOT/dist}"
STAGE="$DIST_DIR/Ottrin-$VERSION-linux-x86_64"
ARCHIVE="$DIST_DIR/Ottrin-$VERSION-linux-x86_64-portable.tar.gz"

PDFIUM_URL="https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/7961/pdfium-linux-x64.tgz"
PDFIUM_SHA256="019665c8877d46fe65f625f80fd714ab07aac68554b0636acf2a2adf9288adb2"
SOURCE_REVISION="$(git -C "$ROOT" rev-parse --verify HEAD)"

if ! git -C "$ROOT" diff --quiet --ignore-submodules -- \
  || ! git -C "$ROOT" diff --cached --quiet --ignore-submodules --; then
    printf 'Refusing to package a tracked-dirty checkout; commit or stash tracked changes first.\n' >&2
    exit 1
fi

download_verify_and_package_pdfium() {
    local target_pdfium_dir="$1"
    local tmp_dir
    tmp_dir="$(mktemp -d)"
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
    rm -rf "$tmp_dir"
}

cd "$ROOT"
cargo build --release -p ottrin-app -p ottrin-platform -p ottrin-search

rm -rf "$STAGE"
mkdir -p "$STAGE/bin" "$STAGE/assets" "$STAGE/docs" "$STAGE/packaging/linux"

install -m 0755 "$BUILD_DIR/ottrin-app" "$STAGE/bin/ottrin"
install -m 0755 "$BUILD_DIR/ottrin-priv-helper" "$STAGE/bin/ottrin-priv-helper"
install -m 0755 "$BUILD_DIR/ottrin-index" "$STAGE/bin/ottrin-index"
download_verify_and_package_pdfium "$STAGE/bin/pdfium"
cp "$ROOT/assets/icon.svg" "$ROOT/assets/icon.png" "$ROOT/assets/logo.svg" "$STAGE/assets/"
cp "$ROOT/packaging/linux/org.ottrin.filesystem.policy" \
  "$ROOT/packaging/linux/ottrin.appdata.xml" "$STAGE/packaging/linux/"
cp "$ROOT/README.md" "$ROOT/LICENSE" "$STAGE/" 2>/dev/null || cp "$ROOT/README.md" "$STAGE/"
cp "$ROOT/docs/USER_GUIDE.md" "$ROOT/docs/DEVELOPMENT.md" "$ROOT/docs/ROADMAP.md" \
  "$ROOT/docs/RELEASE_NOTES.md" "$ROOT/docs/RELEASE_CHECKLIST.md" "$STAGE/docs/"
cat > "$STAGE/SOURCE_REVISION.txt" <<EOF
Ottrin source revision
Version: $VERSION
Git revision: $SOURCE_REVISION
EOF

cat > "$STAGE/RUNNING.md" <<'EOF'
# Running Ottrin Portable on Linux

Run:

```sh
./bin/ottrin
```

Optional privileged helper setup:

```sh
sudo install -m 0755 ./bin/ottrin-priv-helper /usr/libexec/ottrin-priv-helper
sudo install -m 0644 ./packaging/linux/org.ottrin.filesystem.policy /usr/share/polkit-1/actions/
```

Without the helper, normal user-owned file operations still work.

EOF

tar -C "$DIST_DIR" -czf "$ARCHIVE" "$(basename "$STAGE")"
sha256sum "$ARCHIVE" | sed "s|$ARCHIVE|$(basename "$ARCHIVE")|" > "$ARCHIVE.sha256"

printf '%s\n' "$ARCHIVE"
