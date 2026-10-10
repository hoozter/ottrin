#!/usr/bin/env bash
set -euo pipefail
umask 022

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$ROOT/packaging/package-lifecycle.sh"
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
WORK="$(mktemp -d "$DIST_DIR/.macos.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
APP="$WORK/Ottrin.app"
ARCHIVE="$DIST_DIR/Ottrin-$VERSION-macos-$ARCH.zip"

"$ROOT/packaging/macos/stage-app-bundle.sh" "$APP" >/dev/null
(
  cd "$WORK"
  COPYFILE_DISABLE=1 zip -qry "$WORK/$(basename "$ARCHIVE")" "$(basename "$APP")"
)
ottrin_publish_artifact "$WORK/$(basename "$ARCHIVE")" "$ARCHIVE"
if [[ "$(uname -s)" == Darwin ]]; then
  shasum -a 256 "$ARCHIVE" | sed "s|$ARCHIVE|$(basename "$ARCHIVE")|" > "$ARCHIVE.sha256"
else
  sha256sum "$ARCHIVE" | sed "s|$ARCHIVE|$(basename "$ARCHIVE")|" > "$ARCHIVE.sha256"
fi
printf '%s\n' "$ARCHIVE"
