#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PACKAGER="$ROOT/packaging/macos/package-app.sh"
BUILD_ALL="$ROOT/build-all.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
BIN="$TMP/bin"
TOOLS="$TMP/tools"
DIST="$TMP/dist"
mkdir -p "$BIN" "$TOOLS"

for executable in ottrin-app ottrin-indexd; do
  printf '#!/bin/sh\nexit 0\n' > "$BIN/$executable"
  chmod 0755 "$BIN/$executable"
done

printf '#!/bin/sh\nprintf "Darwin\\n"\n' > "$TOOLS/uname"
printf '#!/bin/sh\nexit 99\n' > "$TOOLS/sha256sum"
cat > "$TOOLS/shasum" <<'EOF'
#!/bin/sh
test "$1" = -a && test "$2" = 256
shift 2
exec /usr/bin/sha256sum "$@"
EOF
chmod 0755 "$TOOLS/uname" "$TOOLS/sha256sum" "$TOOLS/shasum"

output="$(
  OTTRIN_BUILD_DIR="$BIN" \
  OTTRIN_DIST_DIR="$DIST" \
  OTTRIN_MACOS_ARCH=arm64 \
  OTTRIN_SKIP_BUILD=1 \
  OTTRIN_SOURCE_REVISION=test-revision \
  PATH="$TOOLS:$PATH" \
    "$PACKAGER"
)"
archive="$DIST/Ottrin-0.1.0-macos-arm64.zip"

test "$output" = "$archive"
test -f "$archive"
test -f "$archive.sha256"
(cd "$DIST" && sha256sum -c "$(basename "$archive.sha256")")
unzip -Z1 "$archive" | grep -Fqx 'Ottrin.app/Contents/MacOS/Ottrin'
unzip -Z1 "$archive" | grep -Fqx 'Ottrin.app/Contents/MacOS/ottrin-indexd'
unzip -Z1 "$archive" | grep -Fqx 'Ottrin.app/Contents/Info.plist'
unzip -p "$archive" Ottrin.app/Contents/Resources/SOURCE_REVISION.txt \
  | grep -Fqx 'Git revision: test-revision'
grep -Fq 'stage-app-bundle.sh' "$PACKAGER"
grep -Fq 'packaging/macos/package-app.sh' "$BUILD_ALL"

printf 'macOS app archive contract: PASS\n'
