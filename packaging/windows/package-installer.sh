#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION="${OTTRIN_VERSION:-$(grep -m1 '^version = ' "$ROOT/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')}"
DIST_DIR="${OTTRIN_DIST_DIR:-$ROOT/dist}"
STAGE="${1:-$DIST_DIR/Ottrin-$VERSION-windows-x64}"
INSTALLER="$DIST_DIR/Ottrin-$VERSION-windows-x64-setup.exe"
MAKENSIS="${MAKENSIS:-}"

if [[ ! -d "$STAGE" ]]; then
  printf 'Windows installer stage does not exist: %s\n' "$STAGE" >&2
  printf 'Build it first with packaging/windows/package-portable.sh\n' >&2
  exit 1
fi
STAGE="$(cd "$STAGE" && pwd)"

required=(
  bin/ottrin.exe
  bin/ottrin-index.exe
  bin/ottrin-priv-helper.exe
  bin/pdfium/pdfium.dll
  bin/pdfium/LICENSE
  bin/pdfium/licenses/pdfium.txt
  bin/libEGL.dll
  bin/libGLESv2.dll
  bin/libgallium_wgl.dll
  bin/dxil.dll
  third-party-notices/MANIFEST.txt
  third-party-notices/mesa3d-26.1.3/docs/license.rst
  third-party-notices/mesa3d-26.1.3/licenses/MIT
  third-party-notices/pal1000-LICENSE.txt
  third-party-notices/Microsoft-DXC-LICENSE-MS.txt
  README.md
  docs/USER_GUIDE.md
  docs/RELEASE_NOTES.md
  docs/RELEASE_CHECKLIST.md
)
for relative in "${required[@]}"; do
  if [[ ! -f "$STAGE/$relative" ]]; then
    printf 'Installer payload is incomplete: missing %s\n' "$relative" >&2
    exit 1
  fi
done

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

mkdir -p "$DIST_DIR"
rm -f "$INSTALLER" "$INSTALLER.sha256"
"$MAKENSIS" \
  "-DVERSION=$VERSION" \
  "-DSTAGE=$STAGE" \
  "-DOUTPUT=$INSTALLER" \
  "$ROOT/packaging/windows/installer.nsi"

if [[ ! -f "$INSTALLER" ]]; then
  printf 'NSIS did not produce the expected installer: %s\n' "$INSTALLER" >&2
  exit 1
fi
sha256sum "$INSTALLER" | sed "s|$INSTALLER|$(basename "$INSTALLER")|" > "$INSTALLER.sha256"
printf '%s\n' "$INSTALLER"
