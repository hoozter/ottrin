#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORKFLOW="${1:-$ROOT/.github/workflows/release.yml}"

# The Linux Windows-portable job extracts the checksum-pinned Mesa archive with
# `7z`. Keep that binary an explicit job dependency rather than relying on the
# mutable hosted-runner image.
windows_portable_job="$(awk '
  /^  package-windows-portable:/ { in_job = 1 }
  in_job { print }
  in_job && /^  package-windows-installer:/ { exit }
' "$WORKFLOW")"

if ! grep -Eq '(^|[[:space:]])p7zip-full($|[[:space:]])' <<<"$windows_portable_job"; then
  printf 'Windows portable release job must install p7zip-full for pinned Mesa extraction\n' >&2
  exit 1
fi

windows_installer_job="$(awk '
  /^  package-windows-installer:/ { in_job = 1 }
  in_job { print }
  in_job && /^  release:/ { exit }
' "$WORKFLOW")"

require_installer_literal() {
  local literal="$1"
  if ! grep -Fq -- "$literal" <<<"$windows_installer_job"; then
    printf 'Windows installer release job is missing portable-artifact integrity guard: %s\n' \
      "$literal" >&2
    exit 1
  fi
}

# The installer job receives a portable ZIP from a separate job.  Fail before
# extraction unless the transfer contains exactly one archive and its sidecar
# checksum still matches, rather than silently selecting an arbitrary file.
require_installer_literal '$archives = @(Get-ChildItem dist\Ottrin-*-windows-x64-portable.zip -File)'
require_installer_literal 'if ($archives.Count -ne 1) { throw "Expected exactly one Windows portable archive for installer packaging." }'
require_installer_literal '$checksumFiles = @(Get-ChildItem dist\Ottrin-*-windows-x64-portable.zip.sha256 -File)'
require_installer_literal 'if ($checksumFiles.Count -ne 1) { throw "Expected exactly one Windows portable checksum for installer packaging." }'
require_installer_literal 'if ($expectedHash -ne $actualHash) { throw "Windows portable archive checksum does not match its sidecar." }'
require_installer_literal 'if ($stages.Count -ne 1) { throw "Expected exactly one extracted Windows portable stage for installer packaging." }'
