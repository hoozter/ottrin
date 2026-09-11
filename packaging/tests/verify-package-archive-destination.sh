#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
packagers=(
  "$ROOT/packaging/linux/package-portable.sh"
  "$ROOT/packaging/linux/package-appimage.sh"
  "$ROOT/packaging/linux/package-deb.sh"
  "$ROOT/packaging/linux/package-rpm.sh"
  "$ROOT/packaging/windows/package-portable.sh"
  "$ROOT/packaging/windows/package-installer.sh"
  "$ROOT/packaging/windows/package-msi.sh"
)

for packager in "${packagers[@]}"; do
  grep -Fq '"$DIST_DIR/archive"' "$packager" || {
    printf 'package archive destination is not dist/archive: %s\n' "$packager" >&2
    exit 1
  }
done

printf 'Package archive destination: PASS\n'
