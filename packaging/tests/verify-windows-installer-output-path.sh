#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PACKAGER="$ROOT/packaging/windows/package-installer.sh"

require_literal() {
  local literal="$1"
  if ! grep -Fq -- "$literal" "$PACKAGER"; then
    printf 'Windows installer packager is missing required output-path guard: %s\n' "$literal" >&2
    exit 1
  fi
}

# NSIS resolves /DOUTPUT relative to its script process. Normalize an override
# before constructing the output name so isolated relative artifact directories
# are valid just like the default absolute repository dist directory.
require_literal 'DIST_DIR="$(cd "$DIST_DIR" && pwd)"'

printf 'Verified Windows installer output-path normalization.\n'
