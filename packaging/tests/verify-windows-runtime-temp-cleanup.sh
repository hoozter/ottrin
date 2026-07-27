#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BASH_PACKAGER="$ROOT/packaging/windows/package-portable.sh"
POWERSHELL_PACKAGER="$ROOT/packaging/windows/package-portable.ps1"

require_literal() {
  local file="$1"
  local literal="$2"
  if ! grep -Fq -- "$literal" "$file"; then
    printf 'missing portable-runtime temporary-directory cleanup in %s: %s\n' "$file" "$literal" >&2
    exit 1
  fi
}

require_near_function() {
  local file="$1"
  local start="$2"
  local literal="$3"
  if ! sed -n "/$start/,+35p" "$file" | grep -Fq -- "$literal"; then
    printf 'missing portable-runtime temporary-directory cleanup near %s in %s: %s\n' "$start" "$file" "$literal" >&2
    exit 1
  fi
}

# PDFium is downloaded before the final archive exists.  A failed download or
# extraction must not leave stale unverified bytes for a subsequent package run.
require_literal "$BASH_PACKAGER" 'download_verify_and_package_pdfium() ('
require_near_function "$BASH_PACKAGER" 'download_verify_and_package_pdfium() (' \
  'trap '\''rm -rf "$tmp_dir"'\'' EXIT'

require_literal "$POWERSHELL_PACKAGER" 'function Resolve-PdfiumRuntime {'
for literal in \
  'try {' \
  'finally {' \
  'Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue'; do
  require_near_function "$POWERSHELL_PACKAGER" \
    'function Resolve-PdfiumRuntime {' "$literal"
done
