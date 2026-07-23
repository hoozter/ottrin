#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION="${OTTRIN_VERSION:-$(grep -m1 '^version = ' "$ROOT/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')}"
BUILD_DIR="$ROOT/target/release"
DIST_DIR="$ROOT/dist"
STAGE="$DIST_DIR/Ottrin-$VERSION-linux-x86_64"
ARCHIVE="$DIST_DIR/Ottrin-$VERSION-linux-x86_64-portable.tar.gz"

cd "$ROOT"
cargo build --release -p ottrin-app -p ottrin-platform -p ottrin-search

rm -rf "$STAGE"
mkdir -p "$STAGE/bin" "$STAGE/assets" "$STAGE/docs" "$STAGE/packaging/linux"

install -m 0755 "$BUILD_DIR/ottrin-app" "$STAGE/bin/ottrin"
install -m 0755 "$BUILD_DIR/ottrin-priv-helper" "$STAGE/bin/ottrin-priv-helper"
install -m 0755 "$BUILD_DIR/ottrin-index" "$STAGE/bin/ottrin-index"
cp "$ROOT/assets/icon.svg" "$ROOT/assets/icon.png" "$ROOT/assets/logo.svg" "$STAGE/assets/"
cp "$ROOT/packaging/linux/org.ottrin.filesystem.policy" \
  "$ROOT/packaging/linux/ottrin.appdata.xml" "$STAGE/packaging/linux/"
cp "$ROOT/README.md" "$ROOT/LICENSE" "$STAGE/" 2>/dev/null || cp "$ROOT/README.md" "$STAGE/"
cp "$ROOT/docs/USER_GUIDE.md" "$ROOT/docs/DEVELOPMENT.md" "$ROOT/docs/ROADMAP.md" \
  "$ROOT/docs/RELEASE_NOTES.md" "$ROOT/docs/RELEASE_CHECKLIST.md" "$STAGE/docs/"

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
