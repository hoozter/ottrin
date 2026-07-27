#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BASH_PACKAGER="$ROOT/packaging/windows/package-installer.sh"
POWERSHELL_PACKAGER="$ROOT/packaging/windows/package-installer.ps1"

require_literal() {
  local file="$1"
  local literal="$2"
  if ! grep -Fq -- "$literal" "$file"; then
    printf 'Windows installer packager must require the exact staged package-version line: %s\n' "$literal" >&2
    exit 1
  fi
}

# The installer filename is derived from this checkout's package version. A
# matching Git revision alone is not enough if a malformed or hand-edited
# portable stage records a different Version line: reject it before NSIS can
# produce a misleading versioned installer.
require_literal "$BASH_PACKAGER" 'grep -Fqx "Version: $VERSION" "$STAGE/SOURCE_REVISION.txt"'
require_literal "$POWERSHELL_PACKAGER" '((Get-Content (Join-Path $Stage "SOURCE_REVISION.txt")) -contains "Version: $Version")'

printf 'Verified Windows installer staged package-version guards.\n'
