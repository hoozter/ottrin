#!/usr/bin/env bash
# Sign native RPM headers before generating release checksums or detached signatures.
set -euo pipefail

fingerprint="${OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT:-}"
if (( $# == 0 )) || [[ ! "$fingerprint" =~ ^[0-9A-Fa-f]{40}$ ]]; then
  printf 'Usage: OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT=<primary fingerprint> %s RPM...\n' "${0##*/}" >&2
  exit 2
fi
gpg_path="$(command -v gpg)"
for package in "$@"; do
  identity="$(rpm -qp --queryformat '%{NAME} %{ARCH}' -- "$package")"
  if [[ "$identity" != 'ottrin x86_64' ]]; then
    printf 'Refusing to sign a package other than Ottrin x86_64: %s\n' "$package" >&2
    exit 2
  fi
  rpmsign --define "__gpg $gpg_path" --define "_gpg_name $fingerprint"     --addsign -- "$package"
done
