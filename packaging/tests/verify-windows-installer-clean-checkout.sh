#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

require_literal() {
  local packager="$1"
  local literal="$2"
  if ! grep -Fq -- "$literal" "$packager"; then
    printf 'Windows installer packager must reject a tracked-dirty checkout: %s (%s)\n' \
      "$literal" "$packager" >&2
    exit 1
  fi
}

# SOURCE_REVISION.txt proves the portable stage identity, but NSIS source files
# are read from this checkout while producing the installer.  Both entry points
# must reject dirty tracked inputs before they can claim that provenance.
require_literal "$ROOT/packaging/windows/package-installer.sh" \
  'git -C "$ROOT" diff --quiet --ignore-submodules --'
require_literal "$ROOT/packaging/windows/package-installer.sh" \
  'git -C "$ROOT" diff --cached --quiet --ignore-submodules --'
require_literal "$ROOT/packaging/windows/package-installer.ps1" \
  '& git -C $Root diff --quiet --ignore-submodules --'
require_literal "$ROOT/packaging/windows/package-installer.ps1" \
  '& git -C $Root diff --cached --quiet --ignore-submodules --'

printf 'Verified Windows installer clean-checkout guards.\n'
