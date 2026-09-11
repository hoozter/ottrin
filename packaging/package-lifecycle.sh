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

ottrin_archive_predecessor() {
  local artifact="$1" archive_root="$2"
  [[ -e "$artifact" || -L "$artifact" ]] || return 0
  local stamp destination name
  stamp="${OTTRIN_ARCHIVE_STAMP:-$(date -u +%Y%m%dT%H%M%SZ)}"
  name="$(basename "$artifact")"
  destination="$archive_root/$stamp/$name"
  mkdir -p "$(dirname "$destination")"
  if [[ -e "$destination" || -L "$destination" ]]; then
    printf 'Archive destination already exists: %s\n' "$destination" >&2
    return 1
  fi
  mv -- "$artifact" "$destination"
  printf 'Archived predecessor: %s\n' "$destination" >&2
}

ottrin_prepare_artifact() {
  local artifact="$1" archive_root="$2"
  local stamp name predecessor destination
  stamp="${OTTRIN_ARCHIVE_STAMP:-$(date -u +%Y%m%dT%H%M%SZ)}"
  for predecessor in "$artifact" "$artifact.sha256"; do
    [[ -e "$predecessor" || -L "$predecessor" ]] || continue
    name="$(basename "$predecessor")"
    destination="$archive_root/$stamp/$name"
    if [[ -e "$destination" || -L "$destination" ]]; then
      printf 'Archive destination already exists: %s\n' "$destination" >&2
      return 1
    fi
  done
  OTTRIN_ARCHIVE_STAMP="$stamp" ottrin_archive_predecessor "$artifact" "$archive_root"
  OTTRIN_ARCHIVE_STAMP="$stamp" ottrin_archive_predecessor "$artifact.sha256" "$archive_root"
}
