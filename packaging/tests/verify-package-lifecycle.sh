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
OTTRIN_ARCHIVE_STAMP=focused-test ottrin_prepare_artifact "$artifact" "$TMP/archive"
[[ ! -e "$artifact" && ! -e "$artifact.sha256" ]]
[[ "$(<"$TMP/archive/focused-test/ottrin.zip")" == 'old artifact' ]]
[[ "$(<"$TMP/archive/focused-test/ottrin.zip.sha256")" == 'old checksum' ]]

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

collision_artifact="$TMP/collision/ottrin.zip"
mkdir -p "$TMP/collision" "$TMP/collision-archive/focused-test"
printf 'old collision artifact' > "$collision_artifact"
printf 'old collision checksum' > "$collision_artifact.sha256"
printf 'existing archived checksum' > "$TMP/collision-archive/focused-test/ottrin.zip.sha256"
if OTTRIN_ARCHIVE_STAMP=focused-test ottrin_prepare_artifact "$collision_artifact" "$TMP/collision-archive"; then
  printf 'Expected archive collision to fail before moving either predecessor.\n' >&2
  exit 1
fi
[[ "$(<"$collision_artifact")" == 'old collision artifact' ]]
[[ "$(<"$collision_artifact.sha256")" == 'old collision checksum' ]]
[[ ! -e "$TMP/collision-archive/focused-test/ottrin.zip" ]]

printf 'Verified package archive and dirty-worktree provenance lifecycle.\n'