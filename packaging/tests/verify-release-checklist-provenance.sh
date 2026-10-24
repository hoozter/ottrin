#!/usr/bin/env bash
# Documentation fixtures intentionally retain literal Markdown backticks.
# shellcheck disable=SC2016
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CHECKLIST="$ROOT/docs/RELEASE_CHECKLIST.md"
README="$ROOT/README.md"
RELEASE_NOTES="$ROOT/docs/RELEASE_NOTES.md"
USER_GUIDE="$ROOT/docs/USER_GUIDE.md"
ROADMAP="$ROOT/docs/ROADMAP.md"
DEVELOPMENT="$ROOT/docs/DEVELOPMENT.md"
MOVE_RECOVERY_DESIGN="$ROOT/docs/MOVE_RECOVERY_DESIGN.md"

require_line() {
  local expected="$1"
  if ! grep -Fq -- "$expected" "$CHECKLIST"; then
    printf 'release checklist is missing required current-evidence line: %s\n' "$expected" >&2
    exit 1
  fi
}

require_document_line() {
  local document="$1"
  local expected="$2"
  local label="$3"
  if ! grep -Fq -- "$expected" "$document"; then
    printf '%s is missing %s: %s\n' "${document#"$ROOT"/}" "$label" "$expected" >&2
    exit 1
  fi
}

# The checklist may preserve historical evidence, but every current exact-tree,
# artifact, CI, and physical-desktop gate must remain visibly unchecked until it
# is rerun against the release candidate.
require_line 'Historical local gates and artifact inspection do not establish the current'
require_line '- [ ] Exact release artifact checksums verify after rebuilding from the current release commit.'
require_line '- [ ] Archive inspection confirms the exact rebuilt app, indexer, privileged helper, policy/assets, and user documentation are present.'
require_line '- A fresh isolated artifact set was rebuilt and reverified on 2026-07-31, including'
require_line '- [ ] Linux and Windows CI are green for the release commit.'
require_line '- [ ] Portable zip launches on a real Windows desktop.'
require_line '- [ ] Copy, overwrite, rename, multi-select, Trash, permanent delete, and partial-batch warnings pass; Linux same-filesystem and cross-filesystem Move support Skip/Rename/Overwrite/Cancel with explicit complete/partial/recovery outcomes.'
require_line '- [ ] Navigation, file operations, Recycle Bin, multi-select, Tandem View, and status messages pass; same-volume and cross-volume Windows Move satisfy the roadmap contract and fail closed for every still-unsupported conflict or entry-kind boundary.'
require_line '- [ ] Privileged helper allow, cancel, missing-helper, and misconfigured-policy paths pass.'

# Historical evidence may retain checked records. Reject checked tasks only
# outside the explicitly delimited historical section.
if awk '
  /^[[:space:]]*##[[:space:]]+Historical/ { historical = 1; next }
  /^[[:space:]]*##[[:space:]]+/ { historical = 0 }
  !historical && /^[[:space:]]*([-*+]|[0-9]+[.)])[[:space:]]+\[[xX]\]/ { found = 1 }
  END { exit found ? 0 : 1 }
' "$CHECKLIST"; then
  printf 'release checklist contains a checked current-candidate gate without exact-candidate evidence.\n' >&2
  exit 1
fi

# Public documents must describe the pinned compatibility payload and distinguish
# automated exact-artifact evidence from publication and native acceptance. Each
# document has explicit payload, renderer, and evidence boundaries.
require_document_line "$README" 'checksum-pinned Mesa 26.1.3 OpenGL compatibility runtime' 'the pinned Windows payload'
require_document_line "$README" 'no exact-current-commit' 'the artifact non-availability boundary'
require_document_line "$README" 'Exact release artifacts must still verify' 'the exact-artifact gate'
require_document_line "$README" 'Automatic, Native, and OpenGL compatibility' 'the renderer selection contract'

require_document_line "$RELEASE_NOTES" 'checksum-pinned Mesa 26.1.3 OpenGL compatibility runtime' 'the pinned Windows payload'
require_document_line "$RELEASE_NOTES" 'exact-current-commit artifacts are not yet accepted' 'the artifact non-availability boundary'
require_document_line "$RELEASE_NOTES" 'exact rebuilt portable ZIP and installer' 'the exact-artifact gate'
require_document_line "$RELEASE_NOTES" 'Automatic compatibility paths' 'the renderer acceptance contract'

require_document_line "$USER_GUIDE" 'checksum-pinned Mesa 26.1.3 OpenGL compatibility runtime' 'the pinned Windows payload'
require_document_line "$USER_GUIDE" 'Recorded MSI/NSIS candidates have scoped ordinary-user native installation, upgrade, indexing and uninstall checks.' 'the scoped native-candidate evidence boundary'
require_document_line "$USER_GUIDE" 'MSI acceptance is blocked; the broader installer matrix remains incomplete.' 'the failed-rollback and incomplete installer boundary'
require_document_line "$USER_GUIDE" 'Automatic selection, persisted preferences, diagnostic override behavior' 'the renderer acceptance contract'

require_document_line "$ROADMAP" 'checksum-pinned Mesa 26.1.3 OpenGL compatibility runtime' 'the pinned Windows payload'
require_document_line "$ROADMAP" 'exact-artifact verification' 'the exact-artifact gate'
require_document_line "$ROADMAP" 'no current package acceptance is claimed' 'the artifact non-acceptance boundary'
require_document_line "$ROADMAP" 'renderer preferences, diagnostic override' 'the renderer acceptance contract'

for document in "$README" "$RELEASE_NOTES"; do
  for expected in 'renderer opengl' 'OTTRIN_RENDERER=opengl'; do
    if ! grep -Fq -- "$expected" "$document"; then
      printf '%s is missing the explicit Windows graphics override: %s\n' \
        "${document#"$ROOT"/}" "$expected" >&2
      exit 1
    fi
  done
done

# Require canonical non-availability and dormant-recovery statements, then
# reject broader contradictory claims that could otherwise coexist with them.
require_document_line "$README" 'artifact is claimed as published or accepted' 'the current binary non-availability statement'
require_document_line "$RELEASE_NOTES" 'no packaged or runtime recovery capability is claimed' 'the dormant recovery boundary'
require_document_line "$USER_GUIDE" 'dormant/test-only journal and recovery machinery' 'the dormant recovery boundary'
require_document_line "$USER_GUIDE" 'accepted Linux same-filesystem native-rename' 'the accepted Move path boundary'
require_document_line "$USER_GUIDE" 'does not create or depend on them' 'the recovery non-dependency boundary'

# This contract intentionally verifies canonical reviewed statements rather than
# attempting natural-language consistency analysis in shell. Keep narrowly
# identified superseded claims from returning; independent documentation review
# remains responsible for novel contradictory prose.
if grep -Fq -- 'Pre-built binaries for the' "$README"; then
  printf 'README.md again claims unverified current binary availability.\n' >&2
  exit 1
fi
if grep -Fq -- '## Retained Move Recovery' "$USER_GUIDE" \
  || grep -Fq -- 'Startup and batch-visible retained Move recovery cards are now reported' "$RELEASE_NOTES"; then
  printf 'public documentation again presents dormant legacy Move recovery as active.\n' >&2
  exit 1
fi
for document in "$README" "$RELEASE_NOTES" "$USER_GUIDE" "$ROADMAP" "$MOVE_RECOVERY_DESIGN"; do
  if grep -Fq -- 'Move remains disabled for any user-facing workflow' "$document"; then
    printf '%s again hides accepted Linux same-filesystem Move.\n' \
      "${document#"$ROOT"/}" >&2
    exit 1
  fi
done

# The release gate itself must review the release notes and the roadmap must
# describe the packaged graphics compatibility policy. Otherwise a release
# review could approve internally inconsistent artifact documentation.
if ! grep -Fq -- '`docs/RELEASE_NOTES.md`' "$DEVELOPMENT"; then
  printf 'docs/DEVELOPMENT.md is missing release-notes review coverage.\n' >&2
  exit 1
fi

# The roadmap must retain release-owner comparison of the finished notes and
# known limitations inside the exact-candidate gate.
if ! grep -Fq -- 'David reviews the release notes and any remaining known limitations.' "$ROADMAP"; then
  printf 'docs/ROADMAP.md is missing the exact-artifact release-owner review boundary.\n' >&2
  exit 1
fi

# Release review must not leave the public landing page describing the old
# text-only PDF/Office behavior after the packaged visual-preview milestone.
# The portable ZIP and installer both ship this README, so stale wording would
# misstate the exact artifact's Quick Inspector behavior.
for expected in \
  'bounded multi-page PDF previews with Previous/Next/page selection' \
  'bounded XLSX/XLSM and ODS structured worksheet previews' \
  'a bounded native DOCX structure view, and a clearly labeled PPTX extracted-text fallback' \
  'PDF and Office previews retain extracted-text and metadata fallbacks'; do
  if ! grep -Fq -- "$expected" "$README"; then
    printf 'README.md is missing current Quick Inspector release detail: %s\n' \
      "$expected" >&2
    exit 1
  fi
done

# The detailed install guide ships in both Windows artifacts too.  It must not
# regress to the old text-only Office description after the visual-preview
# milestone, otherwise the release review would approve contradictory user
# documentation.
for expected in \
  'bounded multi-page PDF visuals with Previous/Next/page selection' \
  'XLSX/XLSM and ODS use bounded structured worksheet previews' \
  'Admitted DOCX packages use a bounded native structure view' \
  'extracted PDF text and metadata'; do
  if ! grep -Fq -- "$expected" "$USER_GUIDE"; then
    printf 'docs/USER_GUIDE.md is missing current Quick Inspector release detail: %s\n' \
      "$expected" >&2
    exit 1
  fi
done

if grep -Fq -- 'PDF text/metadata, DOCX content, XLSX/XLSM worksheet tables' "$README"; then
  printf 'README.md still advertises the superseded text-only PDF/Office preview summary.\n' >&2
  exit 1
fi

# The notes are shipped inside the release artifacts. They must distinguish
# package integrity from the exact-release-commit CI and physical acceptance
# gates that are still required; otherwise a reader could mistake this
# candidate for a signed-off release.
for expected in \
  'Linux and Windows CI' \
  'real Windows desktop' \
  'packaged Linux polkit' \
  'full packaged-workflow smoke tests'; do
  if ! grep -Fq -- "$expected" "$RELEASE_NOTES"; then
    printf 'release notes are missing the required release-blocker detail: %s\n' "$expected" >&2
    exit 1
  fi
done

# The remaining Windows gate is not a generic launch assertion.  The release
# notes must keep the portable and per-user installer scopes, the native
# default, the explicit bundled-OpenGL recovery route, and UAC/helper coverage
# together so a release owner cannot sign off only one path by accident.
for expected in \
  'portable ZIP and per-user installer' \
  'native WGPU default' \
  '`--renderer opengl` / `OTTRIN_RENDERER=opengl` recovery path' \
  'UAC/helper flows'; do
  if ! grep -Fq -- "$expected" "$RELEASE_NOTES"; then
    printf 'release notes are missing exact Windows acceptance scope: %s\n' "$expected" >&2
    exit 1
  fi
done

# Signing is a release requirement, not a deferred feature. Public notes must
# keep native signing acceptance pending, record the completed source
# integration honestly, and require exact downloaded-artifact verification.
require_document_line "$RELEASE_NOTES" 'Linux and Windows production signing acceptance remains pending' 'the pending signing boundary'
require_document_line "$ROADMAP" 'Linux and Windows code-signing integration is source-complete; production identities and exact downloaded-artifact verification remain release gates.' 'the source-complete signing-integration boundary'
require_document_line "$ROADMAP" 'Sign the exact Linux and Windows release artifacts with the approved production identities where required and verify the downloaded signatures.' 'the exact-artifact signing gate'

printf 'Verified release checklist evidence provenance.\n'
