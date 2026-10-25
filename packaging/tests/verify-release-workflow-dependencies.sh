#!/usr/bin/env bash
# These source-contract fixtures must retain the target language's literal variables.
# shellcheck disable=SC2016
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORKFLOW="${1:-$ROOT/.github/workflows/release.yml}"

# The mandatory Linux Office boundary tests need actual namespace/cgroup support.
for verification in "$WORKFLOW" "${2:-$ROOT/.github/workflows/ci.yml}"; do
  verify_job="$(awk '/^  verify:/ { active=1; next } active && /^  [a-zA-Z_-]+:/ { exit } active { print }' "$verification")"
  for dependency in bubblewrap dbus-user-session apparmor-profiles; do
    if ! grep -Eq "(^|[[:space:]])$dependency($|[[:space:]])" <<<"$verify_job"; then
      printf 'Linux verification omits isolation dependency: %s (%s)\n' "$dependency" "$verification" >&2
      exit 1
    fi
  done
  if ! grep -Fq './scripts/ci/prepare-linux-isolation.sh' <<<"$verify_job"; then
    printf 'Linux verification omits real process-isolation provisioning: %s\n' "$verification" >&2
    exit 1
  fi
done

# The publishing verifier requires all four Linux formats. Keep production,
# transfer, signatures and inspection tools in the same release job contract.
linux_job="$(awk '/^  package-linux:/ { active=1 } active && /^  package-windows-portable:/ { exit } active { print }' "$WORKFLOW")"
for literal in \
  './packaging/linux/package-deb.sh' \
  './packaging/linux/package-rpm.sh' \
  'dist/ottrin_*_amd64.deb' \
  'dist/ottrin_*_amd64.deb.sha256' \
  'dist/ottrin-*.x86_64.rpm' \
  'dist/ottrin-*.x86_64.rpm.sha256'; do
  if ! grep -Fq -- "$literal" <<<"$linux_job"; then
    printf 'Linux release job omits a required native artifact: %s\n' "$literal" >&2
    exit 1
  fi
done
for step in 'Sign Linux release artifacts' 'Verify exact downloaded Linux signatures'; do
  body="$(awk -v step="$step" '
    $0 ~ "      - name: " step "$" { active=1; next }
    active && /^      - / { exit }
    active { print }
  ' "$WORKFLOW")"
  for artifact in 'dist/ottrin_*_amd64.deb' 'dist/ottrin-*.x86_64.rpm'; do
    if ! grep -Fq -- "$artifact" <<<"$body"; then
      printf 'Native Linux artifact missing from %s: %s\n' "$step" "$artifact" >&2
      exit 1
    fi
  done
done
for tool in rpm cpio binutils; do
  for job in "$linux_job" "$(sed -n '/^  release:/,$p' "$WORKFLOW")"; do
    if ! grep -Eq "(^|[[:space:]])$tool($|[[:space:]])" <<<"$job"; then
      printf 'Linux release build/inspection job omits tool: %s\n' "$tool" >&2
      exit 1
    fi
  done
done

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
  'os: [ubuntu-latest, windows-latest]' \
  'needs: package-windows-portable' \
  'needs: package-windows-installer' \
  'needs: [package-linux, verify-windows-artifacts]'; do
  if ! grep -Fq -- "$literal" "$WORKFLOW"; then
    printf 'Release workflow omits the Linux/Windows dependency: %s\n' "$literal" >&2
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

# Deferred macOS must not be pulled back into either the job graph or the
# shared publish-time artifact verifier.
if grep -Eiq 'macos' "$WORKFLOW" "$ROOT/packaging/verify-release-artifacts.sh"; then
  printf 'Deferred macOS is still required by the active release pipeline\n' >&2
  exit 1
fi
