#!/usr/bin/env bash
set -euo pipefail

fingerprint="${OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT:-}"
fingerprint="${fingerprint^^}"
if (( $# < 2 )) || [[ ! "$fingerprint" =~ ^[0-9A-F]{40}$ ]]; then
  printf 'Usage: OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT=<primary fingerprint> %s PUBLIC_KEY RPM...\n' "${0##*/}" >&2
  exit 2
fi
key="$1"
shift
database="$(mktemp -d)"
trap 'rm -rf -- "$database"' EXIT
primary="$(gpg --batch --homedir "$database" --with-colons --show-keys "$key" |
  awk -F: '$1=="pub" {primary=1} $1=="fpr" && primary {print toupper($10); primary=0}')"
if [[ "$primary" != "$fingerprint" ]]; then
  printf 'RPM key file must contain exactly the approved primary identity.\n' >&2
  exit 2
fi
mkdir "$database/rpmdb"
rpmkeys --dbpath "$database/rpmdb" --import "$key"
for package in "$@"; do
  verification="$(LC_ALL=C rpmkeys --dbpath "$database/rpmdb" --checksig --verbose -- "$package")"
  if ! grep -Eq 'Signature, key ID [[:xdigit:]]+: OK$' <<< "$verification"; then
    printf 'RPM has no valid signature from the approved key: %s\n' "$package" >&2
    exit 2
  fi
done
