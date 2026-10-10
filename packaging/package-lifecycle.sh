#!/usr/bin/env bash
# Shared, sourceable lifecycle helpers for local Ottrin package builds.
# Packages may be produced from a dirty working tree; artifacts must say so.

ottrin_package_provenance() {
  local root="$1"
  local revision state diff_hash
  revision="$(git -C "$root" rev-parse --verify HEAD)"
  if git -C "$root" diff --quiet --ignore-submodules -- && \
     git -C "$root" diff --cached --quiet --ignore-submodules --; then
    state=clean
    diff_hash=none
  else
    state=tracked-dirty
    diff_hash="$(git -C "$root" diff --binary HEAD | sha256sum | awk '{print $1}')"
  fi
  printf 'Git revision: %s\nWorking tree: %s\nTracked diff SHA-256: %s\n' \
    "$revision" "$state" "$diff_hash"
}

ottrin_write_source_revision() {
  local root="$1" destination="$2" version="$3"
  {
    printf 'Ottrin source revision\n'
    printf 'Version: %s\n' "$version"
    ottrin_package_provenance "$root"
  } > "$destination"
}

# Build candidates beside their destination so replacement is one filesystem rename.
# A failed build leaves the previous usable artifact and receipts in place.
ottrin_publish_artifact() {
  local candidate="$1" artifact="$2"
  [[ -f "$candidate" && ! -L "$candidate" ]] || {
    printf 'Package candidate is missing or not a regular file: %s\n' "$candidate" >&2
    return 1
  }
  mv -f -- "$candidate" "$artifact"
  rm -f -- "$artifact.sha256" "$artifact.provenance.json"
}
