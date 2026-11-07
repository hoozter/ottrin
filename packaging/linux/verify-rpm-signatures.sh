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
  # Require both digests and a trusted signature through RPM's exit
  # status. The human-readable verbose format differs between RPM releases.
  rpmkeys --dbpath "$database/rpmdb" --define '_pkgverify_level all' \
    --define '_pkgverify_flags 0x0' --checksig -- "$package"
done
