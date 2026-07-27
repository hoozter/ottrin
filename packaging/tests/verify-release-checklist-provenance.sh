#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CHECKLIST="$ROOT/docs/RELEASE_CHECKLIST.md"
README="$ROOT/README.md"
RELEASE_NOTES="$ROOT/docs/RELEASE_NOTES.md"
USER_GUIDE="$ROOT/docs/USER_GUIDE.md"
ROADMAP="$ROOT/docs/ROADMAP.md"
DEVELOPMENT="$ROOT/docs/DEVELOPMENT.md"

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

# The public Windows launch instructions must describe the same pinned recovery
# path as the packaged artifacts. The bundled Mesa runtime is a compatibility
# path, not a silent renderer-default change.
for document in "$README" "$RELEASE_NOTES" "$USER_GUIDE" "$ROADMAP"; do
  for expected in \
    'Mesa 26.1.3' \
    'ottrin.exe --renderer opengl' \
    'OTTRIN_RENDERER=opengl' \
    'third-party-notices/'; do
    if ! grep -Fq -- "$expected" "$document"; then
      printf '%s is missing the Windows graphics recovery detail: %s\n' \
        "${document#$ROOT/}" "$expected" >&2
      exit 1
    fi
  done
done

# The release gate itself must review the release notes and the roadmap must
# describe the packaged graphics compatibility policy. Otherwise a release
# review could approve internally inconsistent artifact documentation.
if ! grep -Fq -- '`docs/RELEASE_NOTES.md`' "$DEVELOPMENT"; then
  printf 'docs/DEVELOPMENT.md is missing release-notes review coverage.\n' >&2
  exit 1
fi

# Release review must not leave the public landing page describing the old
# text-only PDF/Office behavior after the packaged visual-preview milestone.
# The portable ZIP and installer both ship this README, so stale wording would
# misstate the exact artifact's Quick Inspector behavior.
for expected in \
  'bounded first-page PDF visuals' \
  'bounded DOCX/XLSX/XLSM content-preview visuals' \
  'PDF and Office previews retain extracted-text and metadata fallbacks'; do
  if ! grep -Fq -- "$expected" "$README"; then
    printf 'README.md is missing current Quick Inspector release detail: %s\n' \
      "$expected" >&2
    exit 1
  fi
done

if grep -Fq -- 'PDF text/metadata, DOCX content, XLSX/XLSM worksheet tables' "$README"; then
  printf 'README.md still advertises the superseded text-only PDF/Office preview summary.\n' >&2
  exit 1
fi

# Code signing is explicitly deferred from v1.  The shipped release notes must
# state that limitation just as the detailed user guidance does.
if ! grep -Fq -- 'Code signing is not included' "$RELEASE_NOTES"; then
  printf 'release notes are missing the deferred code-signing limitation.\n' >&2
  exit 1
fi

printf 'Verified release checklist evidence provenance.\n'
