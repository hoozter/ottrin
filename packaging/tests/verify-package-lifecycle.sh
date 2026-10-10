#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$ROOT/packaging/package-lifecycle.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

artifact="$TMP/dist/ottrin.zip"
mkdir -p "$(dirname "$artifact")"
printf 'old artifact' > "$artifact"
printf 'old checksum' > "$artifact.sha256"
if ottrin_publish_artifact "$TMP/missing-candidate" "$artifact"; then
  printf 'Expected missing candidate to preserve the usable package.\n' >&2
  exit 1
fi
[[ "$(<"$artifact")" == 'old artifact' ]]
[[ "$(<"$artifact.sha256")" == 'old checksum' ]]
printf 'new artifact' > "$TMP/candidate"
ottrin_publish_artifact "$TMP/candidate" "$artifact"
[[ "$(<"$artifact")" == 'new artifact' ]]
[[ ! -e "$artifact.sha256" && ! -e "$TMP/candidate" && ! -e "$TMP/archive" ]]

mkdir "$TMP/repo"
git -C "$TMP/repo" init -q
git -C "$TMP/repo" config user.email test@example.invalid
git -C "$TMP/repo" config user.name test
printf 'base\n' > "$TMP/repo/source"
git -C "$TMP/repo" add source && git -C "$TMP/repo" commit -qm base
printf 'dirty\n' >> "$TMP/repo/source"
ottrin_write_source_revision "$TMP/repo" "$TMP/provenance" 1.2.3
grep -Fqx 'Version: 1.2.3' "$TMP/provenance"
grep -Fqx 'Working tree: tracked-dirty' "$TMP/provenance"
grep -Eq '^Tracked diff SHA-256: [0-9a-f]{64}$' "$TMP/provenance"

printf 'Verified package replacement, failed-build preservation and dirty-worktree provenance.\n'
