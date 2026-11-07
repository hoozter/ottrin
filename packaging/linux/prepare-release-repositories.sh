#!/usr/bin/env bash
# Build repository snapshots from exact, already-published release assets.
set -euo pipefail

tag="${1:?usage: prepare-release-repositories.sh TAG NEW_OUTPUT_DIRECTORY}"
output="${2:?usage: prepare-release-repositories.sh TAG NEW_OUTPUT_DIRECTORY}"
repository="${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"
scripts="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
if [[ ! "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  printf 'Only stable vMAJOR.MINOR.PATCH releases are eligible.\n' >&2
  exit 2
fi
if [[ -e "$output" || -L "$output" ]]; then
  printf 'The repository output must be a new directory.\n' >&2
  exit 2
fi

# Also check on manual retry. A historic or hidden release is not a candidate
# for moving the stable channel backwards.
release="$(gh api "repos/$repository/releases/tags/$tag")"
latest="$(gh api "repos/$repository/releases/latest" --jq .id)"
if ! jq -e --arg tag "$tag" --arg id "$latest" '
  .tag_name == $tag and (.id | tostring) == $id and
  .draft == false and .prerelease == false and
  (.published_at | type == "string" and length > 0)
' <<< "$release" >/dev/null; then
  printf 'The tag is not the current published stable release.\n' >&2
  exit 2
fi

parent="$(cd -- "$(dirname -- "$output")" && pwd)"
output="$parent/$(basename -- "$output")"
work="$(mktemp -d "$parent/.ottrin-release-repos.XXXXXX")"
trap 'rm -rf -- "$work"' EXIT
version="${tag#v}"
deb="ottrin_${version}_amd64.deb"
rpm="$(jq -er --arg prefix "ottrin-$version-" '
  [.assets[].name | select(startswith($prefix) and endswith(".x86_64.rpm"))]
  | if length == 1 then .[0] else error("Expected one x86_64 release RPM") end
' <<< "$release")"
if [[ ! "$rpm" =~ ^[a-zA-Z0-9._+-]+$ ]]; then
  printf 'Invalid release RPM filename.\n' >&2
  exit 2
fi
key=Ottrin-linux-release-key.asc
assets=("$deb" "$deb.sha256" "$deb.asc" "$rpm" "$rpm.sha256" "$rpm.asc" "$key")
patterns=()
for asset in "${assets[@]}"; do patterns+=(--pattern "$asset"); done
gh release download "$tag" --repo "$repository" --dir "$work" "${patterns[@]}"
for package in "$deb" "$rpm"; do
  digest="$(sha256sum "$work/$package")"
  if [[ "$(cat "$work/$package.sha256")" != "${digest%% *}  $package" ]]; then
    printf 'Release checksum mismatch: %s\n' "$package" >&2
    exit 1
  fi
done
"$scripts/verify-release-signatures.sh" "$work/$key" "$work/$deb" "$work/$rpm"
if [[ "$(dpkg-deb -f "$work/$deb" Version)" != "$version" ||
      "$(rpm -qp --queryformat '%{VERSION}' "$work/$rpm")" != "$version" ]]; then
  printf 'Native package versions must match the published release tag.\n' >&2
  exit 2
fi
mkdir "$work/snapshot"
"$scripts/build-apt-repository.sh" "$work/snapshot/apt" "$work/$deb"
"$scripts/build-rpm-repository.sh" "$work/snapshot/rpm" "$work/$rpm"
jq '{id, tag_name, published_at}' <<< "$release" > "$work/snapshot/release.json"
mv -T --no-clobber -- "$work/snapshot" "$output"
test ! -e "$work/snapshot"
