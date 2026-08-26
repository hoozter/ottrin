#!/usr/bin/env bash
set -euo pipefail

usage() {
  printf 'Usage: OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT=<40-hex-fingerprint> %s PUBLIC_KEY ARTIFACT...\n' "${0##*/}" >&2
}

if (($# < 2)); then
  usage
  exit 2
fi

public_key="$1"
shift
expected_fingerprint="${OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT:-}"
expected_fingerprint="${expected_fingerprint//[[:space:]]/}"
expected_fingerprint="${expected_fingerprint^^}"
if [[ ! "$expected_fingerprint" =~ ^[0-9A-F]{40}$ ]]; then
  printf 'OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT must be the approved 40-hex primary-key fingerprint.\n' >&2
  exit 2
fi
if [[ ! -f "$public_key" ]]; then
  printf 'Linux release public key does not exist: %s\n' "$public_key" >&2
  exit 1
fi

owns_gnupg_home=false
if [[ -z "${GNUPGHOME:-}" ]]; then
  GNUPGHOME="$(mktemp -d)"
  owns_gnupg_home=true
fi
export GNUPGHOME
mkdir -p "$GNUPGHOME"
chmod 700 "$GNUPGHOME"

status=""
cleanup() {
  if [[ -n "$status" ]]; then
    rm -f -- "$status"
  fi
  if [[ "$owns_gnupg_home" == true ]]; then
    gpgconf --kill gpg-agent >/dev/null 2>&1 || true
    rm -rf -- "$GNUPGHOME"
  fi
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

gpg --batch --import "$public_key" >/dev/null 2>&1

imported_fingerprints="$(gpg --batch --with-colons --fingerprint "$expected_fingerprint" 2>/dev/null | awk -F: '$1 == "fpr" { print toupper($10) }')"
if ! grep -Fxq "$expected_fingerprint" <<<"$imported_fingerprints"; then
  printf 'The downloaded public key does not contain the approved Linux signing identity.\n' >&2
  exit 1
fi

for artifact in "$@"; do
  signature="$artifact.asc"
  if [[ ! -f "$artifact" || ! -f "$signature" ]]; then
    printf 'Artifact or detached signature is missing: %s\n' "$artifact" >&2
    exit 1
  fi

  status="$(mktemp)"
  if ! gpg --batch --status-fd=1 --verify "$signature" "$artifact" >"$status" 2>/dev/null; then
    printf 'Detached signature verification failed for %s.\n' "$artifact" >&2
    exit 1
  fi
  if ! awk -v expected="$expected_fingerprint" '
    $1 == "[GNUPG:]" && $2 == "VALIDSIG" && (toupper($3) == expected || toupper($12) == expected) { valid = 1 }
    END { exit valid ? 0 : 1 }
  ' "$status"; then
    printf 'Detached signature for %s does not match the approved signing identity.\n' "$artifact" >&2
    exit 1
  fi
  rm -f -- "$status"
  status=""
done
