#!/usr/bin/env bash
# Source-contract fixtures intentionally retain literal target-shell variables.
# shellcheck disable=SC2016
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORKFLOW="$ROOT/.github/workflows/release.yml"
LINUX_SIGNER="$ROOT/packaging/linux/sign-release-artifacts.sh"
LINUX_VERIFIER="$ROOT/packaging/linux/verify-release-signatures.sh"
WINDOWS_SIGNER="$ROOT/packaging/windows/sign-release-artifacts.ps1"
MACOS_SIGNER="$ROOT/packaging/macos/sign-and-notarize-release.sh"
MACOS_VERIFIER="$ROOT/packaging/macos/verify-release-signatures.sh"
PREFLIGHT="$ROOT/scripts/release-preflight.sh"

fail() {
  printf 'Release signing integration contract failed: %s\n' "$1" >&2
  exit 1
}

[[ -x "$LINUX_SIGNER" ]] || fail "Linux signer is missing or not executable"
[[ -x "$LINUX_VERIFIER" ]] || fail "Linux verifier is missing or not executable"
[[ -f "$WINDOWS_SIGNER" ]] || fail "Windows signer is missing"
[[ -x "$MACOS_SIGNER" ]] || fail "macOS signer is missing or not executable"
[[ -x "$MACOS_VERIFIER" ]] || fail "macOS verifier is missing or not executable"

for literal in \
  'OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT' \
  'gpg --batch --armor --detach-sign' \
  'VALIDSIG'; do
  grep -Fq -- "$literal" "$LINUX_SIGNER" || fail "Linux signer omits: $literal"
done

for literal in \
  'OTTRIN_WINDOWS_SIGNING_CERTIFICATE_BASE64' \
  'OTTRIN_WINDOWS_SIGNING_CERTIFICATE_PASSWORD' \
  'Import-PfxCertificate' \
  'signtool.exe' \
  'verify' \
  'Remove-Item'; do
  grep -Fqi -- "$literal" "$WINDOWS_SIGNER" || fail "Windows signer omits: $literal"
done

for literal in \
  'OTTRIN_MACOS_SIGNING_IDENTITY' \
  'OTTRIN_MACOS_TEAM_ID' \
  'codesign' \
  '--keychain "$KEYCHAIN"' \
  'notarytool submit' \
  'stapler staple' \
  'hdiutil create'; do
  grep -Fq -- "$literal" "$MACOS_SIGNER" || fail "macOS signer omits: $literal"
done

grep -Eq '(^|[[:space:]])mapfile([[:space:]]|$)' "$MACOS_VERIFIER" && \
  fail "macOS verifier requires mapfile, which macOS Bash 3.2 does not provide"

for literal in \
  'OTTRIN_MACOS_TEAM_ID' \
  'codesign --verify' \
  'stapler validate' \
  'spctl'; do
  grep -Fq -- "$literal" "$MACOS_VERIFIER" || fail "macOS verifier omits: $literal"
done

for literal in \
  'LINUX_SIGNING_PRIVATE_KEY' \
  'LINUX_SIGNING_KEY_FINGERPRINT' \
  './packaging/linux/sign-release-artifacts.sh' \
  'dist/*.asc' \
  'windows-unsigned-stage' \
  'windows-release-artifacts' \
  'verify-windows-artifacts:' \
  'needs: [package-linux, verify-windows-artifacts]' \
  'pattern: "*-release-artifacts"'; do
  grep -Fq -- "$literal" "$WORKFLOW" || fail "release workflow omits: $literal"
done

# Direct Windows downloads intentionally remain unsigned. Require transfer
# integrity checks, and reject a return to paid-certificate publication gates.
for literal in \
  'Upload unsigned Windows release artifacts' \
  'Verify exact downloaded artifact checksums' \
  'Downloaded checksum mismatch' \
  'Get-FileHash -Algorithm SHA256 -LiteralPath $file.FullName'; do
  grep -Fq -- "$literal" "$WORKFLOW" || fail "unsigned Windows release omits: $literal"
done
if grep -Eq 'WINDOWS_SIGNING_|Get-AuthenticodeSignature|signtool|sign-release-artifacts.ps1' "$WORKFLOW"; then
  fail "direct Windows release still requires Authenticode signing"
fi

for script in \
  packaging/linux/sign-release-artifacts.sh \
  packaging/linux/verify-release-signatures.sh \
  packaging/macos/sign-and-notarize-release.sh \
  packaging/macos/verify-release-signatures.sh \
  packaging/tests/verify-release-signing-integration.sh; do
  sed -n '/^shell_scripts=(/,/^)$/p' "$PREFLIGHT" | grep -Fqx "  $script" || \
    fail "release preflight omits shell syntax coverage for $script"
done

grep -Fq 'Release signing integration contract' "$PREFLIGHT" || \
  fail "release preflight does not execute this contract"

printf 'Verified release signing integration contract.\n'
