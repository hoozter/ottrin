#!/usr/bin/env bash
# Source-contract fixtures intentionally retain literal PowerShell variables.
# shellcheck disable=SC2016
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PACKAGER="$ROOT/packaging/windows/package-installer.ps1"

require_literal() {
  local literal="$1"
  if ! grep -Fq -- "$literal" "$PACKAGER"; then
    printf 'Windows PowerShell installer packager must require the exact staged source revision line: %s\n' "$literal" >&2
    exit 1
  fi
}

# The Linux packager already uses grep -Fqx. The Windows CI packager must not
# accept a staged provenance line that merely contains the expected revision.
require_literal '((Get-Content (Join-Path $Stage "SOURCE_REVISION.txt")) -contains "Git revision: $SourceRevision")'

printf 'Verified Windows PowerShell installer exact provenance guard.\n'
