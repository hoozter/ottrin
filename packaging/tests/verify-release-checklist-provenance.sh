#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CHECKLIST="$ROOT/docs/RELEASE_CHECKLIST.md"
README="$ROOT/README.md"

require_line() {
  local expected="$1"
  if ! grep -Fq -- "$expected" "$CHECKLIST"; then
    printf 'release checklist is missing required current-evidence line: %s\n' "$expected" >&2
    exit 1
  fi
}

# The checklist must distinguish refreshed artifact verification from the
# external CI and physical-desktop gates that remain unproven.
require_line 'Local automated gates and artifact inspection last passed on 2026-07-27.'
require_line '- [ ] Linux and Windows CI are green for the release commit.'
require_line '- [ ] Portable zip launches on a real Windows desktop.'
require_line '- [ ] Navigation, file operations, Recycle Bin, multi-select, Tandem View, and status messages pass; Move rejects without mutation.'
require_line '- [ ] Privileged helper allow, cancel, missing-helper, and misconfigured-policy paths pass.'

# The public Windows launch instructions must retain the explicit recovery
# selector.  The bundled Mesa runtime is a compatibility path, not a silent
# renderer-default change.
if ! grep -Fq -- 'ottrin.exe --renderer opengl' "$README"; then
  printf 'README is missing the explicit Windows OpenGL recovery selector.\n' >&2
  exit 1
fi

printf 'Verified release checklist evidence provenance.\n'
