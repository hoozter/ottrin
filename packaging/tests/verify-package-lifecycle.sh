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

receipt="$TMP/dist/LINUX_PACKAGES_ACCEPTANCE.json"
for filename in \
  Ottrin-1.2.3-x86_64.AppImage \
  Ottrin-1.2.3-linux-x86_64-portable.tar.gz \
  ottrin_1.2.3_amd64.deb \
  ottrin-1.2.3-1.x86_64.rpm; do
  artifact="$TMP/dist/$filename"
  printf 'old artifact' > "$artifact"
  printf 'old acceptance' > "$receipt"
  if ottrin_publish_artifact "$TMP/missing-candidate" "$artifact"; then
    printf 'Expected failed Linux publication to preserve acceptance.\n' >&2
    exit 1
  fi
  [[ "$(<"$receipt")" == 'old acceptance' ]]
  printf 'new artifact' > "$TMP/candidate"
  ottrin_publish_artifact "$TMP/candidate" "$artifact"
  [[ "$(<"$artifact")" == 'new artifact' ]]
  if [[ -e "$receipt" ]]; then
    printf 'Stale Linux cohort acceptance survived package replacement: %s\n' "$filename" >&2
    exit 1
  fi
done

# A failed rename must not erase the previous package's evidence, even when
# the caller handles failure in a conditional rather than relying on errexit.
printf 'old checksum' > "$artifact.sha256"
printf 'old provenance' > "$artifact.provenance.json"
printf 'old acceptance' > "$receipt"
printf 'unpublished artifact' > "$TMP/candidate"
(
  mv() { return 23; }
  if ottrin_publish_artifact "$TMP/candidate" "$artifact"; then
    printf 'Expected a failed artifact rename to fail publication.\n' >&2
    exit 1
  fi
)
[[ "$(<"$artifact")" == 'new artifact' ]]
[[ "$(<"$artifact.sha256")" == 'old checksum' ]]
[[ "$(<"$artifact.provenance.json")" == 'old provenance' ]]
[[ "$(<"$receipt")" == 'old acceptance' ]]
[[ "$(<"$TMP/candidate")" == 'unpublished artifact' ]]

ottrin_publish_artifact "$TMP/candidate" "$TMP/dist/Ottrin-1.2.3-windows-x64-portable.zip"
[[ "$(<"$receipt")" == 'old acceptance' ]]

mkdir "$TMP/repo"
git -C "$TMP/repo" init -q
git -C "$TMP/repo" config user.email test@example.invalid
git -C "$TMP/repo" config user.name test
printf 'base\n' > "$TMP/repo/source"
git -C "$TMP/repo" add source && git -C "$TMP/repo" commit -qm base

# Exercise the actual AppImage publication tail with a tracked cohort receipt.
# Its invalidation must not relabel the embedded clean snapshot as dirty.
mkdir -p "$TMP/repo/dist/packages"
printf 'old acceptance' > "$TMP/repo/dist/packages/LINUX_PACKAGES_ACCEPTANCE.json"
git -C "$TMP/repo" add dist/packages/LINUX_PACKAGES_ACCEPTANCE.json
git -C "$TMP/repo" commit -qm acceptance
appimage_tail="$TMP/appimage-tail.sh"
sed -n '/^# The embedded record is the source snapshot\./,$p' \
  "$ROOT/packaging/linux/package-appimage.sh" > "$appimage_tail"
[[ -s "$appimage_tail" ]]
for cleanliness in clean tracked-dirty; do
  if [[ "$cleanliness" == tracked-dirty ]]; then
    printf 'dirty\n' >> "$TMP/repo/source"
  fi
  (
    ROOT="$TMP/repo"
    APPDIR="$TMP/appdir"
    WORK="$TMP/appimage-work"
    APPIMAGE="$ROOT/dist/packages/Ottrin-1.2.3-x86_64.AppImage"
    mkdir -p "$APPDIR/usr/share/doc/ottrin" "$WORK"
    ottrin_write_source_revision "$ROOT" "$APPDIR/usr/share/doc/ottrin/SOURCE_REVISION.txt" 1.2.3
    printf 'new image %s' "$cleanliness" > "$WORK/$(basename "$APPIMAGE")"
    source "$appimage_tail"
    python3 - "$APPIMAGE" "$cleanliness" "$(git -C "$ROOT" rev-parse HEAD)" <<'PY'
import hashlib,json,pathlib,sys
artifact=pathlib.Path(sys.argv[1])
record=json.loads(pathlib.Path(str(artifact)+'.provenance.json').read_text())
assert record['revision']==sys.argv[3]
assert record['sha256']==hashlib.sha256(artifact.read_bytes()).hexdigest()
assert record['built_from_tracked_clean_source']==(sys.argv[2]=='clean')
PY
  )
  [[ ! -e "$TMP/repo/dist/packages/LINUX_PACKAGES_ACCEPTANCE.json" ]]
  if [[ "$cleanliness" == clean ]]; then
    git -C "$TMP/repo" add -u dist/packages/LINUX_PACKAGES_ACCEPTANCE.json
    git -C "$TMP/repo" commit -qm 'retire replaced cohort'
  fi
done
ottrin_write_source_revision "$TMP/repo" "$TMP/provenance" 1.2.3
grep -Fqx 'Version: 1.2.3' "$TMP/provenance"
grep -Fqx 'Working tree: tracked-dirty' "$TMP/provenance"
grep -Eq '^Tracked diff SHA-256: [0-9a-f]{64}$' "$TMP/provenance"

printf 'Verified package replacement, Linux receipt invalidation, failed-publication preservation and dirty-worktree provenance.\n'
