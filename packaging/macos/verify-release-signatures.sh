#!/usr/bin/env bash
set -euo pipefail

: "${OTTRIN_MACOS_TEAM_ID:?OTTRIN_MACOS_TEAM_ID is required}"
[[ "$(uname -s)" == Darwin ]] || {
  echo 'macOS signature verification must run on macOS.' >&2
  exit 2
}
[[ "$OTTRIN_MACOS_TEAM_ID" =~ ^[A-Z0-9]{10}$ ]] || {
  echo 'OTTRIN_MACOS_TEAM_ID must be a ten-character Apple team identifier.' >&2
  exit 2
}

DIST_DIR="${1:-dist}"
archives=()
while IFS= read -r -d '' artifact; do
  archives+=("$artifact")
done < <(find "$DIST_DIR" -maxdepth 1 -type f -name 'Ottrin-*-macos-*.zip' -print0)
images=()
while IFS= read -r -d '' artifact; do
  images+=("$artifact")
done < <(find "$DIST_DIR" -maxdepth 1 -type f -name 'Ottrin-*-macos-*.dmg' -print0)
[[ "${#archives[@]}" -eq 1 ]] || {
  echo 'Expected exactly one macOS ZIP release artifact.' >&2
  exit 1
}
[[ "${#images[@]}" -eq 1 ]] || {
  echo 'Expected exactly one macOS DMG release artifact.' >&2
  exit 1
}

verify_checksum() {
  local artifact="$1"
  local sidecar="$artifact.sha256"
  [[ -s "$sidecar" ]] || {
    printf 'Missing checksum sidecar: %s\n' "$sidecar" >&2
    exit 1
  }
  (cd "$(dirname "$artifact")" && shasum -a 256 -c "$(basename "$sidecar")")
}
verify_team() {
  local artifact="$1"
  local signature
  signature="$(codesign -dv --verbose=4 "$artifact" 2>&1)"
  grep -Fqx "TeamIdentifier=$OTTRIN_MACOS_TEAM_ID" <<<"$signature" || {
    printf 'Unexpected Apple team identifier on %s\n' "$artifact" >&2
    exit 1
  }
}

ARCHIVE="${archives[0]}"
DMG="${images[0]}"
verify_checksum "$ARCHIVE"
verify_checksum "$DMG"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/ottrin-macos-verify.XXXXXX")"
trap 'rm -rf -- "$WORK"' EXIT HUP INT TERM
ditto -x -k "$ARCHIVE" "$WORK"
APP="$WORK/Ottrin.app"
[[ -d "$APP" ]] || {
  echo 'macOS ZIP does not contain Ottrin.app.' >&2
  exit 1
}

codesign --verify --deep --strict --verbose=2 "$APP"
verify_team "$APP"
xcrun stapler validate "$APP"
spctl --assess --type execute --verbose=2 "$APP"

codesign --verify --strict --verbose=2 "$DMG"
verify_team "$DMG"
xcrun stapler validate "$DMG"
spctl --assess --type open --context context:primary-signature --verbose=2 "$DMG"
printf 'Verified downloaded macOS release signatures and notarization.\n'
