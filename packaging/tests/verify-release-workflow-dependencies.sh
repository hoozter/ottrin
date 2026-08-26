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

for literal in \
  'os: [ubuntu-latest, windows-latest, macos-latest]' \
  'package-macos:' \
  'runs-on: macos-latest' \
  './packaging/macos/package-app.sh' \
  'dist/Ottrin-*-macos-*.zip' \
  'verify-macos-signatures:' \
  'needs: package-windows-portable' \
  'needs: package-windows-installer' \
  'needs: [package-linux, verify-windows-signatures, verify-macos-signatures]'; do
  if ! grep -Fq -- "$literal" "$WORKFLOW"; then
    printf 'Release workflow omits the macOS source-package path: %s\n' "$literal" >&2
    exit 1
  fi
done

# Release jobs handle production signing credentials and a write-capable release
# token. Pin every external action by immutable commit and keep write permission
# on the publication job only.
for literal in \
  'contents: read' \
  'uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262' \
  'uses: dtolnay/rust-toolchain@6c977a6ca4077a0ceb28ffbe03f59d46e9ac8772' \
  'uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6' \
  'uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02' \
  'uses: actions/download-artifact@d3f86a106a0bac45b974a628896c90dbdf5c8093' \
  'uses: softprops/action-gh-release@3bb12739c298aeb8a4eeaf626c5b8d85266b0e65'; do
  if ! grep -Fq -- "$literal" "$WORKFLOW"; then
    printf 'Release workflow is missing its immutable action/permission guard: %s\n' "$literal" >&2
    exit 1
  fi
done

if [[ "$(grep -Fc 'contents: write' "$WORKFLOW")" -ne 1 ]]; then
  printf 'Release workflow must grant contents: write to exactly one job\n' >&2
  exit 1
fi
