#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="${1:-dist}"
VERSION="$(grep -m1 '^version = ' "$ROOT/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')"
MESA_LIBEGL_SHA256="ce36d60c5602920df49a2c8939d903da05dfe2610bfbb4bf0676e9ec3453df3b"
MESA_LIBGLESV2_SHA256="649f779b1b945433dfc2740e9ff1366e309686043999f7271e550079eadde8e6"
MESA_LIBGALLIUM_WGL_SHA256="a3917a52cc893f614c9e415f5442eb2981dd46ba636fc64b35d5bd4be0faec7b"

find_one() {
  local pattern="$1"
  local label="$2"
  local -a matches=()
  mapfile -t matches < <(find "$DIST_DIR" -maxdepth 1 -type f -name "$pattern" -print)
  if (( ${#matches[@]} != 1 )); then
    printf 'expected exactly one %s, found %d\n' "$label" "${#matches[@]}" >&2
    exit 1
  fi
  printf '%s\n' "${matches[0]}"
}

LINUX_ARCHIVE="$(find_one "Ottrin-$VERSION-linux-x86_64-portable.tar.gz" 'Linux portable archive')"
LINUX_APPIMAGE="$(find_one "Ottrin-$VERSION-x86_64.AppImage" 'Linux AppImage')"
WINDOWS_ARCHIVE="$(find_one "Ottrin-$VERSION-windows-x64-portable.zip" 'Windows portable archive')"
WINDOWS_INSTALLER="$(find_one "Ottrin-$VERSION-windows-x64-setup.exe" 'Windows installer')"

expect_artifact_name() {
  local artifact="$1"
  local expected="$2"
  if [[ "$(basename "$artifact")" != "$expected" ]]; then
    printf 'release artifact version/name mismatch: expected %s, found %s\n' \
      "$expected" "$(basename "$artifact")" >&2
    exit 1
  fi
}

expect_artifact_name "$LINUX_ARCHIVE" "Ottrin-$VERSION-linux-x86_64-portable.tar.gz"
expect_artifact_name "$LINUX_APPIMAGE" "Ottrin-$VERSION-x86_64.AppImage"
expect_artifact_name "$WINDOWS_ARCHIVE" "Ottrin-$VERSION-windows-x64-portable.zip"
expect_artifact_name "$WINDOWS_INSTALLER" "Ottrin-$VERSION-windows-x64-setup.exe"

for artifact in "$LINUX_ARCHIVE" "$LINUX_APPIMAGE" "$WINDOWS_ARCHIVE" "$WINDOWS_INSTALLER"; do
  checksum="$artifact.sha256"
  if [[ ! -s "$checksum" ]]; then
    printf 'release artifact checksum is missing: %s\n' "$checksum" >&2
    exit 1
  fi
  checksum_target="$(awk 'NF { print $2 }' "$checksum")"
  checksum_target="${checksum_target#\\*}"
  if [[ "$checksum_target" != "$(basename "$artifact")" ]] \
    || [[ "$(awk 'NF { count++ } END { print count + 0 }' "$checksum")" -ne 1 ]]; then
    printf 'checksum must contain exactly one entry for %s: %s\n' \
      "$(basename "$artifact")" "$checksum" >&2
    exit 1
  fi
  (
    cd "$(dirname "$artifact")"
    sha256sum -c "$(basename "$checksum")"
  )
done

LINUX_CONTENTS="$(tar -tzf "$LINUX_ARCHIVE")"
WINDOWS_CONTENTS="$(unzip -Z1 "$WINDOWS_ARCHIVE")"

require_tar_entry() {
  local pattern="$1"
  if ! grep -Eq "$pattern" <<<"$LINUX_CONTENTS"; then
    echo "Linux portable archive is missing: $pattern" >&2
    exit 1
  fi
}

require_zip_entry() {
  local pattern="$1"
  if ! grep -Eq "$pattern" <<<"$WINDOWS_CONTENTS"; then
    echo "Windows portable archive is missing: $pattern" >&2
    exit 1
  fi
}

for entry in \
  '/bin/ottrin$' \
  '/bin/ottrin-index$' \
  '/bin/ottrin-priv-helper$' \
  '/bin/pdfium/lib/libpdfium\.so$' \
  '/bin/pdfium/LICENSE$' \
  '/bin/pdfium/licenses/pdfium\.txt$' \
  '/assets/icon\.svg$' \
  '/packaging/linux/org\.ottrin\.filesystem\.policy$' \
  '/README\.md$' \
  '/docs/USER_GUIDE\.md$' \
  '/docs/DEVELOPMENT\.md$' \
  '/docs/ROADMAP\.md$' \
  '/docs/RELEASE_NOTES\.md$' \
  '/docs/RELEASE_CHECKLIST\.md$' \
  '/packaging/linux/ottrin\.appdata\.xml$'; do
  require_tar_entry "$entry"
done

for entry in \
  '/bin/ottrin\.exe$' \
  '/bin/ottrin-index\.exe$' \
  '/bin/ottrin-priv-helper\.exe$' \
  '/bin/pdfium/pdfium\.dll$' \
  '/bin/pdfium/LICENSE$' \
  '/bin/pdfium/licenses/pdfium\.txt$' \
  '/bin/libEGL\.dll$' \
  '/bin/libGLESv2\.dll$' \
  '/bin/libgallium_wgl\.dll$' \
  '/bin/dxil\.dll$' \
  '/third-party-notices/MANIFEST\.txt$' \
  '/third-party-notices/mesa3d-26\.1\.3/docs/license\.rst$' \
  '/third-party-notices/mesa3d-26\.1\.3/licenses/MIT$' \
  '/third-party-notices/pal1000-LICENSE\.txt$' \
  '/third-party-notices/Microsoft-DXC-LICENSE-MS\.txt$' \
  '/assets/icon\.svg$' \
  '/README\.md$' \
  '/docs/USER_GUIDE\.md$' \
  '/docs/DEVELOPMENT\.md$' \
  '/docs/ROADMAP\.md$' \
  '/docs/RELEASE_NOTES\.md$' \
  '/docs/RELEASE_CHECKLIST\.md$'; do
  require_zip_entry "$entry"
done

if ! grep -Fq "<release version=\"$VERSION\"" "$ROOT/packaging/linux/ottrin.appdata.xml"; then
  printf 'AppStream metadata does not contain release version %s\n' "$VERSION" >&2
  exit 1
fi

if [[ "$(head -n1 "$ROOT/docs/RELEASE_NOTES.md")" != "# Ottrin $VERSION Release Notes" ]]; then
  printf 'release notes heading does not match package version %s\n' "$VERSION" >&2
  exit 1
fi

for relative_path in \
  'README.md' \
  'docs/USER_GUIDE.md' \
  'docs/DEVELOPMENT.md' \
  'docs/ROADMAP.md' \
  'docs/RELEASE_NOTES.md' \
  'docs/RELEASE_CHECKLIST.md'; do
  if ! cmp -s \
    <(tar -xOf "$LINUX_ARCHIVE" "Ottrin-$VERSION-linux-x86_64/$relative_path") \
    "$ROOT/$relative_path"; then
    printf 'Linux portable archive contains stale content: %s\n' "$relative_path" >&2
    exit 1
  fi
  if ! cmp -s \
    <(unzip -p "$WINDOWS_ARCHIVE" "Ottrin-$VERSION-windows-x64/$relative_path") \
    "$ROOT/$relative_path"; then
    printf 'Windows portable archive contains stale content: %s\n' "$relative_path" >&2
    exit 1
  fi
done

verify_portable_mesa_dll() {
  local expected="$1"
  local entry="$2"
  local actual
  actual="$(unzip -p "$WINDOWS_ARCHIVE" "Ottrin-$VERSION-windows-x64/$entry" | sha256sum | awk '{print $1}')"
  if [[ "$actual" != "$expected" ]]; then
    printf 'Windows portable archive contains an unexpected Mesa DLL: %s\n' "$entry" >&2
    exit 1
  fi
}

verify_portable_mesa_dll "$MESA_LIBEGL_SHA256" 'bin/libEGL.dll'
verify_portable_mesa_dll "$MESA_LIBGLESV2_SHA256" 'bin/libGLESv2.dll'
verify_portable_mesa_dll "$MESA_LIBGALLIUM_WGL_SHA256" 'bin/libgallium_wgl.dll'

for artifact in "$LINUX_APPIMAGE" "$WINDOWS_INSTALLER"; do
  if [[ ! -s "$artifact" ]]; then
    printf 'release artifact is empty: %s\n' "$artifact" >&2
    exit 1
  fi
done

if ! command -v 7z >/dev/null 2>&1; then
  printf '7z is required to inspect the Windows installer payload.\n' >&2
  exit 1
fi

if ! INSTALLER_CONTENTS="$(7z l "$WINDOWS_INSTALLER")"; then
  printf 'Windows installer is not a readable NSIS archive: %s\n' "$WINDOWS_INSTALLER" >&2
  exit 1
fi

require_installer_entry() {
  local entry="$1"
  if ! awk -v entry="$entry" '$NF == entry { found = 1 } END { exit !found }' \
    <<<"$INSTALLER_CONTENTS"; then
    printf 'Windows installer is missing: %s\n' "$entry" >&2
    exit 1
  fi
}

for entry in \
  'bin/ottrin.exe' \
  'bin/ottrin-index.exe' \
  'bin/ottrin-priv-helper.exe' \
  'bin/pdfium/pdfium.dll' \
  'bin/libEGL.dll' \
  'bin/libGLESv2.dll' \
  'bin/libgallium_wgl.dll' \
  'bin/dxil.dll' \
  'third-party-notices/MANIFEST.txt' \
  'third-party-notices/mesa3d-26.1.3/docs/license.rst' \
  'third-party-notices/mesa3d-26.1.3/licenses/MIT' \
  'third-party-notices/pal1000-LICENSE.txt' \
  'third-party-notices/Microsoft-DXC-LICENSE-MS.txt' \
  'docs/USER_GUIDE.md' \
  'docs/RELEASE_NOTES.md' \
  'docs/RELEASE_CHECKLIST.md'; do
  require_installer_entry "$entry"
done

for relative_path in \
  'docs/USER_GUIDE.md' \
  'docs/RELEASE_NOTES.md' \
  'docs/RELEASE_CHECKLIST.md'; do
  if ! 7z x -so "$WINDOWS_INSTALLER" "$relative_path" | cmp -s - "$ROOT/$relative_path"; then
    printf 'Windows installer contains stale content: %s\n' "$relative_path" >&2
    exit 1
  fi
done

verify_installer_mesa_dll() {
  local expected="$1"
  local entry="$2"
  local actual
  actual="$(7z x -so "$WINDOWS_INSTALLER" "$entry" | sha256sum | awk '{print $1}')"
  if [[ "$actual" != "$expected" ]]; then
    printf 'Windows installer contains an unexpected Mesa DLL: %s\n' "$entry" >&2
    exit 1
  fi
}

verify_installer_mesa_dll "$MESA_LIBEGL_SHA256" 'bin/libEGL.dll'
verify_installer_mesa_dll "$MESA_LIBGLESV2_SHA256" 'bin/libGLESv2.dll'
verify_installer_mesa_dll "$MESA_LIBGALLIUM_WGL_SHA256" 'bin/libgallium_wgl.dll'

APPIMAGE_EXTRACT_DIR="$(mktemp -d)"
cleanup() {
  rm -rf -- "$APPIMAGE_EXTRACT_DIR"
}
trap cleanup EXIT

chmod +x "$LINUX_APPIMAGE"
LINUX_APPIMAGE_ABS="$(realpath "$LINUX_APPIMAGE")"
(
  cd "$APPIMAGE_EXTRACT_DIR"
  "$LINUX_APPIMAGE_ABS" --appimage-extract >/dev/null
)

for entry in \
  'AppRun' \
  'usr/bin/ottrin' \
  'usr/bin/ottrin-index' \
  'usr/libexec/ottrin/ottrin-priv-helper' \
  'usr/bin/pdfium/lib/libpdfium.so' \
  'usr/bin/pdfium/LICENSE' \
  'usr/bin/pdfium/licenses/pdfium.txt' \
  'usr/share/applications/org.ottrin.Ottrin.desktop' \
  'usr/share/metainfo/org.ottrin.Ottrin.appdata.xml' \
  'usr/share/polkit-1/actions/org.ottrin.filesystem.policy'; do
  if [[ ! -e "$APPIMAGE_EXTRACT_DIR/squashfs-root/$entry" ]]; then
    printf 'Linux AppImage is missing: %s\n' "$entry" >&2
    exit 1
  fi
done

if [[ "$(tar -tzf "$LINUX_ARCHIVE" | grep -c '/bin/pdfium/lib/libpdfium.so$')" -lt 1 ]]; then
  printf 'Linux portable archive missing PDFium runtime library.\n' >&2
  exit 1
fi

require_zip_entry '/bin/pdfium/pdfium\.dll$'

printf 'Verified release artifacts in %s\n' "$DIST_DIR"
