#!/usr/bin/env bash
set -euo pipefail
umask 077

require_env() {
  local name="$1"
  if [[ -z "${!name:-}" ]]; then
    printf 'Required macOS signing variable is unset: %s\n' "$name" >&2
    exit 2
  fi
}

for name in \
  OTTRIN_MACOS_SIGNING_CERTIFICATE_BASE64 \
  OTTRIN_MACOS_SIGNING_CERTIFICATE_PASSWORD \
  OTTRIN_MACOS_SIGNING_IDENTITY \
  OTTRIN_MACOS_TEAM_ID \
  OTTRIN_MACOS_NOTARIZATION_APPLE_ID \
  OTTRIN_MACOS_NOTARIZATION_PASSWORD; do
  require_env "$name"
done

[[ "$(uname -s)" == Darwin ]] || {
  echo 'macOS release signing must run on macOS.' >&2
  exit 2
}
[[ "$OTTRIN_MACOS_SIGNING_IDENTITY" == 'Developer ID Application: '* ]] || {
  echo 'OTTRIN_MACOS_SIGNING_IDENTITY must name a Developer ID Application identity.' >&2
  exit 2
}
[[ "$OTTRIN_MACOS_TEAM_ID" =~ ^[A-Z0-9]{10}$ ]] || {
  echo 'OTTRIN_MACOS_TEAM_ID must be a ten-character Apple team identifier.' >&2
  exit 2
}

ARCHIVE="${1:-}"
[[ -f "$ARCHIVE" && "$ARCHIVE" == *.zip ]] || {
  echo 'Usage: sign-and-notarize-release.sh path/to/Ottrin-*.zip' >&2
  exit 2
}
ARCHIVE="$(cd "$(dirname "$ARCHIVE")" && pwd)/$(basename "$ARCHIVE")"
DMG="${ARCHIVE%.zip}.dmg"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/ottrin-macos-sign.XXXXXX")"
KEYCHAIN="$WORK/signing.keychain-db"
KEYCHAIN_PASSWORD="$(uuidgen)$(uuidgen)"
CERTIFICATE="$WORK/signing-certificate.p12"

cleanup() {
  security delete-keychain "$KEYCHAIN" >/dev/null 2>&1 || true
  rm -rf -- "$WORK"
}
trap cleanup EXIT HUP INT TERM

printf '%s' "$OTTRIN_MACOS_SIGNING_CERTIFICATE_BASE64" | base64 -D > "$CERTIFICATE"
security create-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN"
security unlock-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN"
security set-keychain-settings -lut 21600 "$KEYCHAIN"
security import "$CERTIFICATE" -k "$KEYCHAIN" \
  -P "$OTTRIN_MACOS_SIGNING_CERTIFICATE_PASSWORD" \
  -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple: -s \
  -k "$KEYCHAIN_PASSWORD" "$KEYCHAIN"
security find-identity -v -p codesigning "$KEYCHAIN" | \
  grep -Fq '"'"$OTTRIN_MACOS_SIGNING_IDENTITY"'"' || {
    echo 'Imported certificate does not provide the requested signing identity.' >&2
    exit 1
  }

mkdir "$WORK/unpacked"
ditto -x -k "$ARCHIVE" "$WORK/unpacked"
APP="$WORK/unpacked/Ottrin.app"
[[ -d "$APP" ]] || {
  echo 'macOS release archive does not contain Ottrin.app.' >&2
  exit 1
}

while IFS= read -r -d '' nested; do
  codesign --force --options runtime --timestamp \
    --keychain "$KEYCHAIN" --sign "$OTTRIN_MACOS_SIGNING_IDENTITY" "$nested"
done < <(find "$APP/Contents" -type f \( -perm -111 -o -name '*.dylib' \) -print0)
codesign --force --options runtime --timestamp \
  --keychain "$KEYCHAIN" --sign "$OTTRIN_MACOS_SIGNING_IDENTITY" "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"

signature="$(codesign -dv --verbose=4 "$APP" 2>&1)"
grep -Fqx "TeamIdentifier=$OTTRIN_MACOS_TEAM_ID" <<<"$signature" || {
  echo 'Signed application team identifier does not match OTTRIN_MACOS_TEAM_ID.' >&2
  exit 1
}

ditto -c -k --keepParent "$APP" "$ARCHIVE.new"
mv -f -- "$ARCHIVE.new" "$ARCHIVE"
xcrun notarytool submit "$ARCHIVE" \
  --apple-id "$OTTRIN_MACOS_NOTARIZATION_APPLE_ID" \
  --team-id "$OTTRIN_MACOS_TEAM_ID" \
  --password "$OTTRIN_MACOS_NOTARIZATION_PASSWORD" \
  --wait
xcrun stapler staple "$APP"
xcrun stapler validate "$APP"

ditto -c -k --keepParent "$APP" "$ARCHIVE.new"
mv -f -- "$ARCHIVE.new" "$ARCHIVE"
rm -f -- "$DMG"
hdiutil create -volname Ottrin -srcfolder "$APP" -ov -format UDZO "$DMG"
codesign --force --timestamp --keychain "$KEYCHAIN" \
  --sign "$OTTRIN_MACOS_SIGNING_IDENTITY" "$DMG"
xcrun notarytool submit "$DMG" \
  --apple-id "$OTTRIN_MACOS_NOTARIZATION_APPLE_ID" \
  --team-id "$OTTRIN_MACOS_TEAM_ID" \
  --password "$OTTRIN_MACOS_NOTARIZATION_PASSWORD" \
  --wait
xcrun stapler staple "$DMG"
xcrun stapler validate "$DMG"

for artifact in "$ARCHIVE" "$DMG"; do
  shasum -a 256 "$artifact" | sed "s|$artifact|$(basename "$artifact")|" > "$artifact.sha256"
done
printf 'Signed and notarized %s and %s\n' "$ARCHIVE" "$DMG"
