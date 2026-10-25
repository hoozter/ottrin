#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$ROOT/packaging/linux/bundle-runtime-libs.sh"
source "$ROOT/packaging/package-lifecycle.sh"
VERSION="${OTTRIN_VERSION:-$(grep -m1 '^version = ' "$ROOT/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')}"
BUILD_DIR="$ROOT/target/release"
DIST_DIR="${OTTRIN_DIST_DIR:-$ROOT/dist}"
mkdir -p "$DIST_DIR"
WORK="$(mktemp -d "$DIST_DIR/.portable.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
STAGE="$WORK/Ottrin-$VERSION-linux-x86_64"
ARCHIVE="$DIST_DIR/Ottrin-$VERSION-linux-x86_64-portable.tar.gz"

source "$ROOT/packaging/linux/package-pdfium.sh"
source "$ROOT/packaging/package-search-runtime.sh"

cd "$ROOT"
if [[ "${OTTRIN_SKIP_BUILD:-0}" != "1" ]]; then
    cargo build --release -p ottrin-app -p ottrin-platform -p ottrin-search
    cargo build --release -p ottrin-preview --bin ottrin-media-playback --features system-gstreamer
    cargo build --release -p ottrin-preview --bin ottrin-legacy-doc-render
fi

rm -rf "$STAGE"
mkdir -p "$STAGE/bin" "$STAGE/libexec/ottrin" "$STAGE/assets" "$STAGE/docs" \
  "$STAGE/packaging/linux"

install -m 0755 "$BUILD_DIR/ottrin-app" "$STAGE/libexec/ottrin/ottrin"
install -m 0755 "$BUILD_DIR/ottrin-legacy-doc-render" "$STAGE/libexec/ottrin/ottrin-legacy-doc-render"
install -m 0755 "$BUILD_DIR/ottrin-media-playback" "$STAGE/libexec/ottrin/ottrin-media-playback"
install -m 0755 "$BUILD_DIR/ottrin-indexd" "$STAGE/libexec/ottrin/ottrin-indexd"
install -m 0755 "$BUILD_DIR/ottrin-priv-helper" "$STAGE/bin/ottrin-priv-helper"
bundle_linux_runtime "$BUILD_DIR/ottrin-app" "$STAGE" "$STAGE/third-party-notices/linux-runtime" \
  "$BUILD_DIR/ottrin-legacy-doc-render"
cat > "$STAGE/bin/ottrin" <<'EOF'
#!/bin/sh
OTTRIN_ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
OTTRIN_REAL_ELF="$OTTRIN_ROOT/libexec/ottrin/ottrin"
export LD_LIBRARY_PATH="$OTTRIN_ROOT/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export OTTRIN_PRIV_HELPER=/usr/libexec/ottrin/ottrin-priv-helper
exec "$OTTRIN_REAL_ELF" "$@"
EOF
chmod 0755 "$STAGE/bin/ottrin"
package_linux_pdfium "$STAGE/libexec/ottrin/pdfium"
package_search_runtime linux-x86_64 "$STAGE/libexec/ottrin" "$STAGE/third-party-notices/ripgrep"
install -D -m 0644 "$ROOT/crates/ottrin-ui/assets/THIRD_PARTY_NOTICES.md" "$STAGE/third-party-notices/BUNDLED_FONTS.md"
cp "$ROOT/assets/icon.svg" "$ROOT/assets/icon.png" "$ROOT/assets/logo.svg" "$STAGE/assets/"
cp "$ROOT/packaging/linux/org.ottrin.filesystem.policy" \
  "$ROOT/packaging/linux/ottrin.appdata.xml" \
  "$ROOT/packaging/linux/ottrin-index.service.in" "$STAGE/packaging/linux/"
cp "$ROOT/README.md" "$ROOT/LICENSE" "$STAGE/" 2>/dev/null || cp "$ROOT/README.md" "$STAGE/"
cp "$ROOT/docs/USER_GUIDE.md" "$ROOT/docs/DEVELOPMENT.md" "$ROOT/docs/ROADMAP.md" \
  "$ROOT/docs/RELEASE_NOTES.md" "$ROOT/docs/RELEASE_CHECKLIST.md" \
  "$ROOT/docs/SEARCH_PERFORMANCE.md" "$STAGE/docs/"
ottrin_write_source_revision "$ROOT" "$STAGE/SOURCE_REVISION.txt" "$VERSION"

cat > "$STAGE/RUNNING.md" <<'EOF'
# Running Ottrin Portable on Linux

Run:

```sh
./bin/ottrin
```

This launcher uses the artifact-local Linux runtime libraries in `./lib/`.

Optional privileged helper setup:

```sh
sudo install -D -m 0755 ./bin/ottrin-priv-helper /usr/libexec/ottrin/ottrin-priv-helper
sudo install -m 0644 ./packaging/linux/org.ottrin.filesystem.policy /usr/share/polkit-1/actions/
```

Without the helper, normal user-owned file operations still work.

Ottrin starts its per-user index daemon when the app launches. To keep the
daemon enabled across login sessions, open Settings → Services and install the
background index service. Ottrin installs the included systemd user-service
template with this package's absolute launcher path; it does not install a
periodic timer or cron job.

EOF

tar -C "$WORK" -czf "$WORK/$(basename "$ARCHIVE")" "$(basename "$STAGE")"
ottrin_publish_artifact "$WORK/$(basename "$ARCHIVE")" "$ARCHIVE"
sha256sum "$ARCHIVE" | sed "s|$ARCHIVE|$(basename "$ARCHIVE")|" > "$ARCHIVE.sha256"

printf '%s\n' "$ARCHIVE"
