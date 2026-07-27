#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PACKAGER="$ROOT/packaging/linux/package-appimage.sh"
VERIFIER="$ROOT/packaging/verify-release-artifacts.sh"

require_literal() {
  local file="$1"
  local expected="$2"
  if ! grep -Fq -- "$expected" "$file"; then
    printf '%s is missing required AppImage documentation contract: %s\n' \
      "${file#$ROOT/}" "$expected" >&2
    exit 1
  fi
}

# The AppImage is a release artifact, so it must carry the same release-facing
# documentation that the portable Linux and Windows packages expose.
for source in \
  '"$ROOT/README.md" "$ROOT/LICENSE" "$APPDIR/usr/share/doc/ottrin/"' \
  '"$ROOT/docs/USER_GUIDE.md" "$ROOT/docs/DEVELOPMENT.md"' \
  '"$ROOT/docs/ROADMAP.md" "$ROOT/docs/RELEASE_NOTES.md"' \
  '"$ROOT/docs/RELEASE_CHECKLIST.md" "$APPDIR/usr/share/doc/ottrin/"'; do
  require_literal "$PACKAGER" "$source"
done

for entry in \
  'usr/share/doc/ottrin/README.md' \
  'usr/share/doc/ottrin/LICENSE' \
  'usr/share/doc/ottrin/USER_GUIDE.md' \
  'usr/share/doc/ottrin/DEVELOPMENT.md' \
  'usr/share/doc/ottrin/ROADMAP.md' \
  'usr/share/doc/ottrin/RELEASE_NOTES.md' \
  'usr/share/doc/ottrin/RELEASE_CHECKLIST.md'; do
  require_literal "$VERIFIER" "'$entry'"
done

printf 'Verified Linux AppImage release-documentation contract.\n'
