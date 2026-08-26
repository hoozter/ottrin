#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
STAGER="$ROOT/packaging/macos/stage-app-bundle.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
BIN="$TMP/bin"
APP="$TMP/Ottrin.app"
mkdir -p "$BIN"

expected_tree_state=clean
if ! git -C "$ROOT" diff --quiet --ignore-submodules -- \
  || ! git -C "$ROOT" diff --cached --quiet --ignore-submodules --; then
  expected_tree_state=dirty
fi

for executable in ottrin-app ottrin-indexd; do
  printf '#!/bin/sh\nexit 0\n' > "$BIN/$executable"
  chmod 0755 "$BIN/$executable"
done

unsafe_destination="$TMP/existing-directory"
mkdir -p "$unsafe_destination"
printf 'preserve\n' > "$unsafe_destination/sentinel"
if OTTRIN_BUILD_DIR="$BIN" OTTRIN_SKIP_BUILD=1 \
  "$STAGER" "$unsafe_destination" >/dev/null 2>&1; then
  printf 'macOS app stager accepted a destination without the .app suffix\n' >&2
  exit 1
fi
grep -Fqx 'preserve' "$unsafe_destination/sentinel"

OTTRIN_BUILD_DIR="$BIN" OTTRIN_SKIP_BUILD=1 OTTRIN_SOURCE_REVISION=test-revision \
  "$STAGER" "$APP"

for executable in Ottrin ottrin-indexd; do
  test -x "$APP/Contents/MacOS/$executable"
done
for payload in \
  Contents/Info.plist \
  Contents/Resources/Ottrin.png \
  Contents/Resources/LICENSE \
  Contents/Resources/README.md \
  Contents/Resources/docs/USER_GUIDE.md \
  Contents/Resources/SOURCE_REVISION.txt; do
  test -f "$APP/$payload"
done

grep -A1 -Fq '<key>CFBundleIdentifier</key>' "$APP/Contents/Info.plist"
grep -Fq '<string>org.ottrin.Ottrin</string>' "$APP/Contents/Info.plist"
grep -A1 -Fq '<key>CFBundleExecutable</key>' "$APP/Contents/Info.plist"
grep -Fq '<string>Ottrin</string>' "$APP/Contents/Info.plist"
grep -A1 -Fq '<key>CFBundleShortVersionString</key>' "$APP/Contents/Info.plist"
grep -Fq '<string>0.1.0</string>' "$APP/Contents/Info.plist"
grep -Fq '<string>NSApplication</string>' "$APP/Contents/Info.plist"
grep -Fqx 'Git revision: test-revision' "$APP/Contents/Resources/SOURCE_REVISION.txt"
grep -Fqx "Tracked tree: $expected_tree_state" "$APP/Contents/Resources/SOURCE_REVISION.txt"

printf 'macOS app-bundle staging contract: PASS\n'
