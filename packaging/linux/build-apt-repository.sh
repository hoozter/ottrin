#!/usr/bin/env bash
# Build a complete signed archive snapshot for staging or local verification.
set -euo pipefail

if (( $# < 2 )); then
  printf 'Usage: %s OUTPUT_DIRECTORY OTTRIN_DEB...\n' "${0##*/}" >&2
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
stage="$(mktemp -d "$parent/.ottrin-apt.XXXXXX")"
trap 'rm -rf -- "$stage"' EXIT
mkdir -p "$stage/pool/main/o/ottrin" "$stage/dists/stable/main/binary-amd64"
for package in "$@"; do
  name="$(dpkg-deb -f "$package" Package)"
  version="$(dpkg-deb -f "$package" Version)"
  arch="$(dpkg-deb -f "$package" Architecture)"
  if [[ "$name" != ottrin || "$arch" != amd64 || ! "$version" =~ ^[0-9][a-zA-Z0-9.+~-]*$ ]]; then
    printf 'Unexpected package identity or version: %s\n' "$package" >&2
    exit 2
  fi
  target="$stage/pool/main/o/ottrin/ottrin_${version}_${arch}.deb"
  if [[ -e "$target" ]]; then
    printf 'Duplicate Ottrin package version: %s\n' "$version" >&2
    exit 2
  fi
  install -m 0644 "$package" "$target"
done

(
  cd -- "$stage"
  index=dists/stable/main/binary-amd64
  apt-ftparchive packages pool > "$index/Packages"
  gzip -n -c "$index/Packages" > "$index/Packages.gz"
  apt-ftparchive \
    -o APT::FTPArchive::Release::Origin=Ottrin \
    -o APT::FTPArchive::Release::Label=Ottrin \
    -o APT::FTPArchive::Release::Suite=stable \
    -o APT::FTPArchive::Release::Codename=stable \
    -o APT::FTPArchive::Release::Architectures=amd64 \
    -o APT::FTPArchive::Release::Components=main \
    -o APT::FTPArchive::Release::Acquire-By-Hash=yes \
    release dists/stable > Release
  mv -- Release dists/stable/Release
  for algorithm in SHA256 SHA512; do
    mkdir -p "$index/by-hash/$algorithm"
    for file in "$index/Packages" "$index/Packages.gz"; do
      digest="$("${algorithm,,}sum" "$file")"
      cp -- "$file" "$index/by-hash/$algorithm/${digest%% *}"
    done
  done
  gpg --batch --local-user "$fingerprint" --digest-algo SHA256 \
    --output dists/stable/InRelease --clearsign dists/stable/Release
  gpg --batch --export-options export-minimal --export "$fingerprint" > ottrin-archive-keyring.gpg
  gpgv --keyring "$PWD/ottrin-archive-keyring.gpg" dists/stable/InRelease
)
chmod 0755 "$stage"
mv -T --no-clobber -- "$stage" "$output"
test ! -e "$stage"
