#!/usr/bin/env bash
set -euo pipefail

usage() {
  printf 'Usage: OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT=<40-hex-fingerprint> %s ARTIFACT...\n' "${0##*/}" >&2
}

if (($# == 0)); then
  usage
  exit 2
fi

expected_fingerprint="${OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT:-}"
expected_fingerprint="${expected_fingerprint//[[:space:]]/}"
expected_fingerprint="${expected_fingerprint^^}"
if [[ ! "$expected_fingerprint" =~ ^[0-9A-F]{40}$ ]]; then
  printf 'OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT must be the approved 40-hex primary-key fingerprint.\n' >&2
  exit 2
fi

if ! command -v gpg >/dev/null 2>&1; then
  printf 'gpg is required to sign Linux release artifacts.\n' >&2
  exit 1
fi

if ! gpg --batch --list-secret-keys "$expected_fingerprint" >/dev/null 2>&1; then
  printf 'The approved Linux release signing secret key is not available.\n' >&2
  exit 1
fi

verify_signature() {
  local artifact="$1"
  local signature="$artifact.asc"
  local status
  status="$(mktemp)"
  trap 'rm -f "$status"' RETURN

  if ! gpg --batch --status-fd=1 --verify "$signature" "$artifact" >"$status" 2>/dev/null; then
    printf 'Detached signature verification failed for %s.\n' "$artifact" >&2
    return 1
  fi

  if ! awk -v expected="$expected_fingerprint" '
    $1 == "[GNUPG:]" && $2 == "VALIDSIG" && (toupper($3) == expected || toupper($12) == expected) { valid = 1 }
    END { exit valid ? 0 : 1 }
  ' "$status"; then
    printf 'Detached signature for %s does not match the approved signing identity.\n' "$artifact" >&2
    return 1
  fi
}

for artifact in "$@"; do
  if [[ ! -f "$artifact" ]]; then
    printf 'Release artifact does not exist: %s\n' "$artifact" >&2
    exit 1
  fi

  gpg --batch --armor --detach-sign --yes \
    --local-user "$expected_fingerprint" \
    --output "$artifact.asc" \
    "$artifact"
  verify_signature "$artifact"
done
