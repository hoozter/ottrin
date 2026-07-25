#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION="${OTTRIN_VERSION:-$(grep -m1 '^version = ' "$ROOT/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')}"
TARGET="${OTTRIN_TARGET:-x86_64-pc-windows-gnu}"
BUILD_DIR="$ROOT/target/$TARGET/release"
DIST_DIR="$ROOT/dist"
STAGE="$DIST_DIR/Ottrin-$VERSION-windows-x64"
ARCHIVE="$DIST_DIR/Ottrin-$VERSION-windows-x64-portable.zip"
PDFIUM_URL="https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/7961/pdfium-win-x64.tgz"
PDFIUM_SHA256="88276459349b291c41f10422dad0210f007c04d919c8fa56472b6b7c6406adf4"

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
    cp -a "$extract_dir/bin/." "$target_pdfium_dir/"
    cp -a "$extract_dir/LICENSE" "$extract_dir/licenses" "$target_pdfium_dir/"
    rm -rf "$tmp_dir"
}

cd "$ROOT"
cargo build --release --target "$TARGET" -p ottrin-app -p ottrin-platform -p ottrin-search

rm -rf "$STAGE"
mkdir -p "$STAGE/bin" "$STAGE/assets" "$STAGE/docs"

cp "$BUILD_DIR/ottrin-app.exe" "$STAGE/bin/ottrin.exe"
cp "$BUILD_DIR/ottrin-priv-helper.exe" "$STAGE/bin/ottrin-priv-helper.exe"
cp "$BUILD_DIR/ottrin-index.exe" "$STAGE/bin/ottrin-index.exe"
download_verify_and_package_pdfium "$STAGE/bin/pdfium"
cp "$ROOT/assets/icon.svg" "$ROOT/assets/icon.png" "$ROOT/assets/logo.svg" "$STAGE/assets/"
cp "$ROOT/README.md" "$STAGE/"
if [ -f "$ROOT/LICENSE" ]; then
  cp "$ROOT/LICENSE" "$STAGE/"
fi
cp "$ROOT/docs/USER_GUIDE.md" "$ROOT/docs/DEVELOPMENT.md" "$ROOT/docs/ROADMAP.md" \
  "$ROOT/docs/RELEASE_NOTES.md" "$ROOT/docs/RELEASE_CHECKLIST.md" "$STAGE/docs/"

cat > "$STAGE/RUNNING.md" <<'EOF'
# Running Ottrin Portable on Windows

Run:

```powershell
.\bin\ottrin.exe
```

The privileged helper is bundled next to the app as:

```text
.\bin\ottrin-priv-helper.exe
```

Normal user-owned file operations do not require elevation. Denied operations use the bundled helper through the Windows UAC flow.
EOF

rm -f "$ARCHIVE"
(cd "$DIST_DIR" && zip -qr "$(basename "$ARCHIVE")" "$(basename "$STAGE")")
sha256sum "$ARCHIVE" | sed "s|$ARCHIVE|$(basename "$ARCHIVE")|" > "$ARCHIVE.sha256"

printf '%s\n' "$ARCHIVE"
