#!/usr/bin/env bash
# Source-contract fixtures intentionally retain literal shell and PowerShell variables.
# shellcheck disable=SC2016
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SOURCE="$ROOT/packaging/windows/installer.wxs"
PACKAGER="$ROOT/packaging/windows/package-msi.sh"
POWERSHELL_PACKAGER="$ROOT/packaging/windows/package-msi.ps1"

require_literal() {
  local file="$1"
  local literal="$2"
  if ! grep -Fq -- "$literal" "$file"; then
    printf 'Windows MSI lifecycle contract is missing: %s (%s)\n' "$literal" "$file" >&2
    exit 1
  fi
}

[[ -f "$SOURCE" ]] || { printf 'Windows MSI source is missing: %s\n' "$SOURCE" >&2; exit 1; }
[[ -x "$PACKAGER" ]] || { printf 'Windows MSI packager is missing or not executable: %s\n' "$PACKAGER" >&2; exit 1; }
[[ -f "$POWERSHELL_PACKAGER" ]] || { printf 'Native Windows MSI packager is missing: %s\n' "$POWERSHELL_PACKAGER" >&2; exit 1; }

# The MSI is per-user and delegates lifecycle ownership to the installed app.
# Cleanup runs before RemoveFiles and a nonzero cleanup result aborts uninstall,
# preserving the installed payload and registration for a retry.
require_literal "$SOURCE" 'Scope="perUser"'
require_literal "$SOURCE" 'Schedule="afterInstallExecute"'
require_literal "$SOURCE" 'ExeCommand="index service-install"'
require_literal "$SOURCE" 'ExeCommand="--uninstall"'
require_literal "$SOURCE" 'Before="RemoveFiles"'
require_literal "$SOURCE" 'Return="check"'
require_literal "$SOURCE" 'Condition="NOT REMOVE"'
require_literal "$SOURCE" 'Condition="REMOVE~=&quot;ALL&quot; AND NOT UPGRADINGPRODUCTCODE"'

# Package identity and payload provenance must come from the exact portable
# stage, using the same release version/source guards as the NSIS candidate.
require_literal "$PACKAGER" 'SOURCE_REVISION.txt'
require_literal "$PACKAGER" 'Git revision: $source_revision'
require_literal "$PACKAGER" '"$WIX" build'
require_literal "$PACKAGER" '-arch x64'
require_literal "$PACKAGER" 'sha256sum'
require_literal "$POWERSHELL_PACKAGER" '& $WixPath build'
require_literal "$POWERSHELL_PACKAGER" '"-arch" "x64"'
require_literal "$POWERSHELL_PACKAGER" 'Get-FileHash -Algorithm SHA256'

printf 'Verified Windows MSI lifecycle and provenance contract.\n'
