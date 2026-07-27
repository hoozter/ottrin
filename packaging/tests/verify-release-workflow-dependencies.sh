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
