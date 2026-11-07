#!/usr/bin/env bash
# Assemble a signed snapshot from RPMs already signed for the GitHub Release.
set -euo pipefail

if (( $# < 2 )); then
  printf 'Usage: %s OUTPUT_DIRECTORY SIGNED_OTTRIN_RPM...\n' "${0##*/}" >&2
  exit 2
fi
output="$1"
shift
fingerprint="${OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT:-}"
if [[ ! "$fingerprint" =~ ^[0-9A-Fa-f]{40}$ ]]; then
  printf 'Set OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT to the approved primary fingerprint.\n' >&2
  exit 2
fi
if [[ -e "$output" || -L "$output" ]]; then
  printf 'The repository output must be a new directory.\n' >&2
  exit 2
fi

parent="$(cd -- "$(dirname -- "$output")" && pwd)"
output="$parent/$(basename -- "$output")"
stage="$(mktemp -d "$parent/.ottrin-rpm.XXXXXX")"
trap 'rm -rf -- "$stage"' EXIT
mkdir -p "$stage/Packages"
key="$stage/ottrin-rpm-key.asc"
gpg --batch --armor --export-options export-minimal --export "$fingerprint" > "$key"
test -s "$key"
"$(dirname -- "${BASH_SOURCE[0]}")/verify-rpm-signatures.sh" "$key" "$@"

for package in "$@"; do
  identity="$(rpm -qp --queryformat '%{NAME} %{ARCH} %{VERSION}-%{RELEASE}' -- "$package")"
  if [[ ! "$identity" =~ ^ottrin\ x86_64\ ([0-9][a-zA-Z0-9.+~_-]*)$ ]]; then
    printf 'Unexpected package identity or version: %s\n' "$package" >&2
    exit 2
  fi
  version_release="${BASH_REMATCH[1]}"
  target="$stage/Packages/ottrin-$version_release.x86_64.rpm"
  if [[ -e "$target" ]]; then
    printf 'Duplicate Ottrin package version and release: %s\n' "$version_release" >&2
    exit 2
  fi
  install -m 0644 "$package" "$target"
done

createrepo_c --unique-md-filenames "$stage"
"$(dirname -- "${BASH_SOURCE[0]}")/sign-release-artifacts.sh" "$stage/repodata/repomd.xml"
chmod 0755 "$stage"
mv -T --no-clobber -- "$stage" "$output"
test ! -e "$stage"
