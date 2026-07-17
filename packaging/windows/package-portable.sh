#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION="${OTTRIN_VERSION:-$(grep -m1 '^version = ' "$ROOT/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')}"
TARGET="${OTTRIN_TARGET:-x86_64-pc-windows-gnu}"
BUILD_DIR="$ROOT/target/$TARGET/release"
DIST_DIR="$ROOT/dist"
STAGE="$DIST_DIR/Ottrin-$VERSION-windows-x64"
ARCHIVE="$DIST_DIR/Ottrin-$VERSION-windows-x64-portable.zip"

cd "$ROOT"
cargo build --release --target "$TARGET" -p ottrin-app -p ottrin-platform -p ottrin-search

rm -rf "$STAGE"
mkdir -p "$STAGE/bin" "$STAGE/assets" "$STAGE/docs"

cp "$BUILD_DIR/ottrin-app.exe" "$STAGE/bin/ottrin.exe"
cp "$BUILD_DIR/ottrin-priv-helper.exe" "$STAGE/bin/ottrin-priv-helper.exe"
cp "$BUILD_DIR/ottrin-index.exe" "$STAGE/bin/ottrin-index.exe"
cp "$ROOT/assets/icon.svg" "$ROOT/assets/icon.png" "$ROOT/assets/logo.svg" "$STAGE/assets/"
cp "$ROOT/README.md" "$STAGE/"
if [ -f "$ROOT/LICENSE" ]; then
  cp "$ROOT/LICENSE" "$STAGE/"
fi
cp "$ROOT/docs/USER_GUIDE.md" "$ROOT/docs/DEVELOPMENT.md" "$ROOT/docs/ROADMAP.md" "$STAGE/docs/"

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
