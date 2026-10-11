#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$ROOT/packaging/package-lifecycle.sh"
VERSION="${OTTRIN_VERSION:-$(grep -m1 '^version = ' "$ROOT/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')}"
DIST_DIR="${OTTRIN_DIST_DIR:-$ROOT/dist}"
STAGE="${1:-$DIST_DIR/windows-payload/Ottrin-$VERSION-windows-x64}"
MAKENSIS="${MAKENSIS:-}"



if [[ ! -d "$STAGE" ]]; then
  printf 'Windows installer stage does not exist: %s\n' "$STAGE" >&2
  printf 'Build it first with packaging/windows/package-portable.sh\n' >&2
  exit 1
fi
STAGE="$(cd "$STAGE" && pwd)"
mkdir -p "$DIST_DIR"
DIST_DIR="$(cd "$DIST_DIR" && pwd)"
INSTALLER="$DIST_DIR/Ottrin-$VERSION-windows-x64-setup.exe"

required=(
  bin/ottrin.exe
  bin/ottrin-office-render.exe
  bin/ottrin-legacy-doc-render.exe
  bin/ottrin-priv-helper.exe
  bin/pdfium/pdfium.dll
  bin/pdfium/LICENSE
  bin/pdfium/licenses/pdfium.txt
  bin/opengl32.dll
  bin/libEGL.dll
  bin/libGLESv2.dll
  bin/libgallium_wgl.dll
  bin/dxil.dll
  third-party-notices/MANIFEST.txt
  third-party-notices/mesa3d-26.1.3/docs/license.rst
  third-party-notices/mesa3d-26.1.3/licenses/Apache-2.0
  third-party-notices/mesa3d-26.1.3/licenses/BSL-1.0
  third-party-notices/mesa3d-26.1.3/licenses/GPL-1.0-or-later
  third-party-notices/mesa3d-26.1.3/licenses/GPL-2.0-only
  third-party-notices/mesa3d-26.1.3/licenses/MIT
  third-party-notices/mesa3d-26.1.3/licenses/SGI-B-2.0
  third-party-notices/mesa3d-26.1.3/licenses/exceptions/Linux-Syscall-Note
  third-party-notices/pal1000-LICENSE.txt
  third-party-notices/Microsoft-DXC-LICENSE-MS.txt
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
  if [[ ! -f "$STAGE/$relative" ]]; then
    printf 'Installer payload is incomplete: missing %s\n' "$relative" >&2
    exit 1
  fi
done

# NSIS copies this stage verbatim.  Keep the release-facing files tied to the
# exact checkout that is producing the installer instead of accepting a stale
# portable stage that happens to have the right filenames.
release_files=(
  LICENSE
  README.md
  docs/USER_GUIDE.md
  docs/DEVELOPMENT.md
  docs/ROADMAP.md
  docs/RELEASE_NOTES.md
  docs/RELEASE_CHECKLIST.md
)
for relative in "${release_files[@]}"; do
  if ! cmp -s "$STAGE/$relative" "$ROOT/$relative"; then
    printf 'Installer payload is stale: %s\n' "$relative" >&2
    exit 1
  fi
done

source_revision="$(git -C "$ROOT" rev-parse --verify HEAD)"
if ! grep -Fqx "Version: $VERSION" "$STAGE/SOURCE_REVISION.txt"; then
  printf 'Installer payload has a stale package version.\n' >&2
  exit 1
fi
if ! grep -Fqx "Git revision: $source_revision" "$STAGE/SOURCE_REVISION.txt"; then
  printf 'Installer payload has a stale source revision.\n' >&2
  exit 1
fi

if [[ -z "$MAKENSIS" ]]; then
  if command -v makensis >/dev/null 2>&1; then
    MAKENSIS="$(command -v makensis)"
  elif [[ -n "${HOME:-}" && -x "$HOME/.local/bin/makensis" ]]; then
    MAKENSIS="$HOME/.local/bin/makensis"
  else
    printf 'makensis was not found. Install NSIS (Ubuntu: sudo apt install nsis) or set MAKENSIS.\n' >&2
    exit 1
  fi
fi

WORK="$(mktemp -d "$DIST_DIR/.windows-installer.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
"$MAKENSIS" \
  "-DVERSION=$VERSION" \
  "-DSTAGE=$STAGE" \
  "-DOUTPUT=$WORK/$(basename "$INSTALLER")" \
  "$ROOT/packaging/windows/installer.nsi"

if [[ ! -f "$WORK/$(basename "$INSTALLER")" ]]; then
  printf 'NSIS did not produce the expected installer: %s\n' "$INSTALLER" >&2
  exit 1
fi
ottrin_publish_artifact "$WORK/$(basename "$INSTALLER")" "$INSTALLER"
sha256sum "$INSTALLER" | sed "s|$INSTALLER|$(basename "$INSTALLER")|" > "$INSTALLER.sha256"
printf '%s\n' "$INSTALLER"
