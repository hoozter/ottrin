#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BASH_LINUX_PORTABLE="$ROOT/packaging/linux/package-portable.sh"
BASH_APPIMAGE="$ROOT/packaging/linux/package-appimage.sh"
BASH_WINDOWS_PORTABLE="$ROOT/packaging/windows/package-portable.sh"
POWERSHELL_WINDOWS_PORTABLE="$ROOT/packaging/windows/package-portable.ps1"
BASH_WINDOWS_INSTALLER="$ROOT/packaging/windows/package-installer.sh"
POWERSHELL_WINDOWS_INSTALLER="$ROOT/packaging/windows/package-installer.ps1"
ARTIFACT_VERIFIER="$ROOT/packaging/verify-release-artifacts.sh"

require_literal() {
  local file="$1"
  local literal="$2"
  if ! grep -Fq -- "$literal" "$file"; then
    printf 'missing release-provenance guard in %s: %s\n' "$file" "$literal" >&2
    exit 1
  fi
}

for packager in "$BASH_LINUX_PORTABLE" "$BASH_APPIMAGE" "$BASH_WINDOWS_PORTABLE"; do
  require_literal "$packager" 'SOURCE_REVISION="$(git -C "$ROOT" rev-parse --verify HEAD)"'
  require_literal "$packager" 'SOURCE_REVISION.txt'
done

for packager in "$POWERSHELL_WINDOWS_PORTABLE"; do
  require_literal "$packager" '$SourceRevision = (& git -C $Root rev-parse --verify HEAD).Trim()'
  require_literal "$packager" 'SOURCE_REVISION.txt'
done

for installer in "$BASH_WINDOWS_INSTALLER" "$POWERSHELL_WINDOWS_INSTALLER"; do
  require_literal "$installer" 'SOURCE_REVISION.txt'
done

for literal in \
  'Git revision: $EXPECTED_SOURCE_REVISION' \
  "'Linux portable archive'" \
  "'Windows portable archive'" \
  "'Windows installer'" \
  "'Linux AppImage'"; do
  require_literal "$ARTIFACT_VERIFIER" "$literal"
done

printf 'Verified release artifact provenance guards.\n'
