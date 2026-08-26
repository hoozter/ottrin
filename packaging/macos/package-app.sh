#!/usr/bin/env bash
set -euo pipefail
umask 022

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION="${OTTRIN_VERSION:-$(grep -m1 '^version = ' "$ROOT/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')}"
DIST_DIR="${OTTRIN_DIST_DIR:-$ROOT/dist}"
ARCH="${OTTRIN_MACOS_ARCH:-$(uname -m)}"
case "$ARCH" in
  arm64|x86_64) ;;
  *)
    printf 'Unsupported macOS package architecture: %s\n' "$ARCH" >&2
    exit 1
    ;;
esac

mkdir -p "$DIST_DIR"
DIST_DIR="$(cd "$DIST_DIR" && pwd)"
APP="$DIST_DIR/Ottrin.app"
ARCHIVE="$DIST_DIR/Ottrin-$VERSION-macos-$ARCH.zip"

"$ROOT/packaging/macos/stage-app-bundle.sh" "$APP" >/dev/null
rm -f -- "$ARCHIVE" "$ARCHIVE.sha256"
(
  cd "$DIST_DIR"
  COPYFILE_DISABLE=1 zip -qry "$(basename "$ARCHIVE")" "$(basename "$APP")"
)
if [[ "$(uname -s)" == Darwin ]]; then
  shasum -a 256 "$ARCHIVE" | sed "s|$ARCHIVE|$(basename "$ARCHIVE")|" > "$ARCHIVE.sha256"
else
  sha256sum "$ARCHIVE" | sed "s|$ARCHIVE|$(basename "$ARCHIVE")|" > "$ARCHIVE.sha256"
fi
printf '%s\n' "$ARCHIVE"
