#!/usr/bin/env bash
# Source-contract fixtures intentionally retain literal target-shell variables.
# shellcheck disable=SC2016
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

require_literal() {
  local packager="$1"
  local literal="$2"
  if ! grep -Fq -- "$literal" "$packager"; then
    printf 'Windows installer packager must record local-worktree provenance: %s (%s)\n' \
      "$literal" "$packager" >&2
    exit 1
  fi
}

# SOURCE_REVISION.txt proves the portable stage identity. Packaging is allowed
# from a dirty working tree only when its provenance explicitly records that
# state instead of falsely calling it a clean HEAD build.
require_literal "$ROOT/packaging/windows/package-installer.sh" \
  'source "$ROOT/packaging/package-lifecycle.sh"'
require_literal "$ROOT/packaging/package-lifecycle.sh" \
  'Working tree: %s'
require_literal "$ROOT/packaging/package-lifecycle.sh" \
  'Tracked diff SHA-256: %s'

printf 'Verified Windows installer local-worktree provenance guards.\n'
