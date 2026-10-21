#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
task_dir="$(mktemp -d)"
trap 'rm -rf -- "$task_dir"' EXIT
fixture="$task_dir/project"
mkdir -p "$fixture/packaging/linux" "$fixture/packaging/tests" "$fixture/dist/packages" "$task_dir/bin"
cp "$ROOT/build-all.sh" "$fixture/build-all.sh"
cp "$ROOT/packaging/package-lifecycle.sh" "$fixture/packaging/"
printf '#!/bin/sh\nexit 0\n' > "$task_dir/bin/rpmbuild"
chmod +x "$task_dir/bin/rpmbuild"
cat > "$task_dir/mock-packager" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
source "$ROOT/packaging/package-lifecycle.sh"
case "${0##*/}" in
  package-portable.sh) kind=portable; name=Ottrin-1.2.3-linux-x86_64-portable.tar.gz ;;
  package-appimage.sh) kind=appimage; name=Ottrin-1.2.3-x86_64.AppImage ;;
  package-deb.sh) kind=deb; name=ottrin_1.2.3_amd64.deb ;;
  package-rpm.sh) kind=rpm; name=ottrin-1.2.3-1.x86_64.rpm ;;
  *) exit 90 ;;
esac
[[ -f "$ROOT/dist/packages/LINUX_PACKAGES_ACCEPTANCE.json" ]] || { echo 'Published before all packages were built' >&2; exit 91; }
if [[ "${OTTRIN_FAIL_KIND:-}" == "$kind" ]]; then exit 29; fi
mkdir -p "$OTTRIN_DIST_DIR"
candidate="$(mktemp "$OTTRIN_DIST_DIR/.candidate.XXXXXX")"
{ printf 'new %s\n' "$kind"; ottrin_package_provenance "$ROOT"; } > "$candidate"
ottrin_publish_artifact "$candidate" "$OTTRIN_DIST_DIR/$name"
sha256sum "$OTTRIN_DIST_DIR/$name" | sed "s|$OTTRIN_DIST_DIR/$name|$name|" > "$OTTRIN_DIST_DIR/$name.sha256"
if [[ "$kind" == appimage ]]; then printf 'source sidecar\n' > "$OTTRIN_DIST_DIR/$name.provenance.json"; fi
if [[ "${OTTRIN_CORRUPT_KIND:-}" == "$kind" ]]; then printf '\ncorrupt\n' >> "$OTTRIN_DIST_DIR/$name"; fi
printf '%s\n' "$OTTRIN_DIST_DIR/$name"
MOCK
for kind in portable appimage deb rpm; do
  install -m 0755 "$task_dir/mock-packager" "$fixture/packaging/linux/package-$kind.sh"
done
cat > "$fixture/packaging/tests/verify-linux-privileged-helper-package-path.sh" <<'VERIFY'
#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
[[ -f "$ROOT/dist/packages/LINUX_PACKAGES_ACCEPTANCE.json" ]]
[[ $# == 4 ]]
for argument in "$@"; do
  candidate="${argument#*=}"
  [[ -f "$candidate" && "$candidate" == *'/.build-all.'*'/linux/'* ]]
  grep -Fqx 'Working tree: clean' "$candidate"
done
VERIFY
chmod +x "$fixture/packaging/tests/verify-linux-privileged-helper-package-path.sh"
printf '/dist/\n' > "$fixture/.gitignore"
printf 'old cohort\n' > "$fixture/dist/packages/LINUX_PACKAGES_ACCEPTANCE.json"
git -C "$fixture" init -q
git -C "$fixture" config user.name test
git -C "$fixture" config user.email test@example.invalid
git -C "$fixture" add .
git -C "$fixture" add -f dist/packages/LINUX_PACKAGES_ACCEPTANCE.json
git -C "$fixture" commit -qm baseline
names=(Ottrin-1.2.3-linux-x86_64-portable.tar.gz Ottrin-1.2.3-x86_64.AppImage ottrin_1.2.3_amd64.deb ottrin-1.2.3-1.x86_64.rpm)
for name in "${names[@]}"; do
  printf 'old %s\n' "$name" > "$fixture/dist/packages/$name"
  printf 'old checksum\n' > "$fixture/dist/packages/$name.sha256"
done
if PATH="$task_dir/bin:$PATH" OTTRIN_FAIL_KIND=deb bash "$fixture/build-all.sh" --linux-only > "$task_dir/failure.log" 2>&1; then
  echo 'Expected the injected later package failure' >&2; exit 1
fi
assert_old_packages() {
  local name
  for name in "${names[@]}"; do
    if [[ "$(<"$fixture/dist/packages/$name")" != "old $name" ]]; then
      echo "A later package failure replaced the usable artifact: $name" >&2; exit 1
    fi
    [[ "$(<"$fixture/dist/packages/$name.sha256")" == 'old checksum' ]]
  done
  [[ "$(<"$fixture/dist/packages/LINUX_PACKAGES_ACCEPTANCE.json")" == 'old cohort' ]]
  [[ -z "$(find "$fixture/dist/packages" -maxdepth 1 -name '.build-all.*' -print)" ]]
}
assert_old_packages
if PATH="$task_dir/bin:$PATH" OTTRIN_CORRUPT_KIND=appimage bash "$fixture/build-all.sh" --linux-only > "$task_dir/corrupt.log" 2>&1; then
  echo 'Expected corrupt staged checksum to prevent publication' >&2; exit 1
fi
assert_old_packages
PATH="$task_dir/bin:$PATH" bash "$fixture/build-all.sh" --linux-only > "$task_dir/success.log" 2>&1
for name in "${names[@]}"; do
  grep -Fqx 'Working tree: clean' "$fixture/dist/packages/$name"
  (cd "$fixture/dist/packages" && sha256sum -c "$name.sha256")
  grep -Fq "  $fixture/dist/packages/$name" "$task_dir/success.log"
done
[[ "$(<"$fixture/dist/packages/Ottrin-1.2.3-x86_64.AppImage.provenance.json")" == 'source sidecar' ]]
[[ ! -e "$fixture/dist/packages/LINUX_PACKAGES_ACCEPTANCE.json" ]]
[[ -z "$(find "$fixture/dist/packages" -maxdepth 1 -name '.build-all.*' -print)" ]]
printf 'Verified staged Linux builds, clean source snapshots, failed-build preservation, publication sidecars and teardown.\n'
