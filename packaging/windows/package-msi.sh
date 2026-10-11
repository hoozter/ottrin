#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$ROOT/packaging/package-lifecycle.sh"
VERSION="${OTTRIN_VERSION:-$(grep -m1 '^version = ' "$ROOT/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')}"
DIST_DIR="${OTTRIN_DIST_DIR:-$ROOT/dist}"
STAGE="${1:-$DIST_DIR/windows-payload/Ottrin-$VERSION-windows-x64}"
WIX="${WIX:-wix}"


if [[ ! -d "$STAGE" ]]; then
  printf 'Windows MSI stage does not exist: %s\n' "$STAGE" >&2
  printf 'Build it first with packaging/windows/package-portable.sh\n' >&2
  exit 1
fi
STAGE="$(cd "$STAGE" && pwd)"
mkdir -p "$DIST_DIR"
DIST_DIR="$(cd "$DIST_DIR" && pwd)"
MSI="$DIST_DIR/Ottrin-$VERSION-windows-x64.msi"

required=(
  bin/ottrin.exe
  bin/ottrin-office-render.exe
  bin/ottrin-legacy-doc-render.exe
  bin/ottrin-priv-helper.exe
  bin/pdfium/pdfium.dll
  bin/opengl32.dll
  bin/libEGL.dll
  bin/libGLESv2.dll
  bin/libgallium_wgl.dll
  bin/dxil.dll
  third-party-notices/MANIFEST.txt
  SOURCE_REVISION.txt
  LICENSE
  RUNNING.md
  README.md
  docs/USER_GUIDE.md
  docs/DEVELOPMENT.md
  docs/ROADMAP.md
  docs/RELEASE_NOTES.md
  docs/RELEASE_CHECKLIST.md
)
for relative in "${required[@]}"; do
  [[ -f "$STAGE/$relative" ]] || {
    printf 'MSI payload is incomplete: missing %s\n' "$relative" >&2
    exit 1
  }
done

for relative in LICENSE README.md docs/USER_GUIDE.md docs/DEVELOPMENT.md \
  docs/ROADMAP.md docs/RELEASE_NOTES.md docs/RELEASE_CHECKLIST.md; do
  if ! cmp -s "$STAGE/$relative" "$ROOT/$relative"; then
    printf 'MSI payload is stale: %s\n' "$relative" >&2
    exit 1
  fi
done

source_revision="$(git -C "$ROOT" rev-parse --verify HEAD)"
grep -Fqx "Version: $VERSION" "$STAGE/SOURCE_REVISION.txt" || {
  printf 'MSI payload has a stale package version.\n' >&2
  exit 1
}
grep -Fqx "Git revision: $source_revision" "$STAGE/SOURCE_REVISION.txt" || {
  printf 'MSI payload has a stale source revision.\n' >&2
  exit 1
}

if ! command -v "$WIX" >/dev/null 2>&1 && [[ ! -x "$WIX" ]]; then
  printf 'wix was not found. Install WiX Toolset 5 or set WIX to wix.exe.\n' >&2
  exit 1
fi

WORK="$(mktemp -d "$DIST_DIR/.windows-installer.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
"$WIX" build \
  -d "Version=$VERSION" \
  -d "Stage=$STAGE" \
  -o "$WORK/$(basename "$MSI")" \
  "$ROOT/packaging/windows/installer.wxs"
[[ -f "$WORK/$(basename "$MSI")" ]] || { printf 'WiX did not produce the expected MSI: %s\n' "$MSI" >&2; exit 1; }
ottrin_publish_artifact "$WORK/$(basename "$MSI")" "$MSI"
sha256sum "$MSI" | sed "s|$MSI|$(basename "$MSI")|" > "$MSI.sha256"
printf '%s\n' "$MSI"
