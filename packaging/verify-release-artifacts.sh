#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="${1:-dist}"
VERSION="$(grep -m1 '^version = ' "$ROOT/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')"
MESA_LIBEGL_SHA256="ce36d60c5602920df49a2c8939d903da05dfe2610bfbb4bf0676e9ec3453df3b"
MESA_LIBGLESV2_SHA256="649f779b1b945433dfc2740e9ff1366e309686043999f7271e550079eadde8e6"
MESA_LIBGALLIUM_WGL_SHA256="a3917a52cc893f614c9e415f5442eb2981dd46ba636fc64b35d5bd4be0faec7b"
DXIL_SHA256="cbcfe883a09fd0ca1f98abdf3a9553b560895e3283a136da82a8381253a169df"
MESA_URL="https://github.com/pal1000/mesa-dist-win/releases/download/26.1.3/mesa3d-26.1.3-release-mingw.7z"
MESA_SHA256="80d5add64254c839b4c784bdab6a2b504e448675604b0fe54a9bce3c543303a7"
MESA_SOURCE_URL="https://gitlab.freedesktop.org/mesa/mesa/-/archive/mesa-26.1.3/mesa-mesa-26.1.3.tar.gz"
MESA_SOURCE_SHA256="4ef7c87feb15966d5645ec55728020632d2f54becb42ce546d4a12bfc5275dd1"
PAL_LICENSE_URL="https://raw.githubusercontent.com/pal1000/mesa-dist-win/26.1.3/LICENSE"
PAL_LICENSE_SHA256="9cd0121dc070f0ac65c1fc266bee74bdbbc22cea4210ae72ae0c1f076448d4cc"
DXC_URL="https://github.com/microsoft/DirectXShaderCompiler/releases/download/v1.9.2602.24/dxc_2026_05_27.zip"
DXC_SHA256="cf658aacf070d3045e31b8f1f8a696c2945f37c1095019481ef7c513368db3b4"

"$ROOT/packaging/tests/verify-release-checklist-provenance.sh"

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
  '/LICENSE$' \
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
  '/third-party-notices/mesa3d-26\.1\.3/licenses/Apache-2\.0$' \
  '/third-party-notices/mesa3d-26\.1\.3/licenses/BSL-1\.0$' \
  '/third-party-notices/mesa3d-26\.1\.3/licenses/GPL-1\.0-or-later$' \
  '/third-party-notices/mesa3d-26\.1\.3/licenses/GPL-2\.0-only$' \
  '/third-party-notices/mesa3d-26\.1\.3/licenses/MIT$' \
  '/third-party-notices/mesa3d-26\.1\.3/licenses/SGI-B-2\.0$' \
  '/third-party-notices/mesa3d-26\.1\.3/licenses/exceptions/Linux-Syscall-Note$' \
  '/third-party-notices/pal1000-LICENSE\.txt$' \
  '/third-party-notices/Microsoft-DXC-LICENSE-MS\.txt$' \
  '/assets/icon\.svg$' \
  '/LICENSE$' \
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

verify_release_file_bytes() {
  local relative_path="$1"
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
}

for relative_path in \
  'LICENSE' \
  'README.md' \
  'docs/USER_GUIDE.md' \
  'docs/DEVELOPMENT.md' \
  'docs/ROADMAP.md' \
  'docs/RELEASE_NOTES.md' \
  'docs/RELEASE_CHECKLIST.md'; do
  verify_release_file_bytes "$relative_path"
done

verify_portable_graphics_runtime_dll() {
  local expected="$1"
  local entry="$2"
  local actual
  actual="$(unzip -p "$WINDOWS_ARCHIVE" "Ottrin-$VERSION-windows-x64/$entry" | sha256sum | awk '{print $1}')"
  if [[ "$actual" != "$expected" ]]; then
    printf 'Windows portable archive contains an unexpected graphics runtime DLL: %s\n' "$entry" >&2
    exit 1
  fi
}

# Presence alone is not enough for the redistributable notices: a stale or
# substituted stage could retain the expected paths while losing the texts
# that correspond to the pinned Mesa source archive.  These hashes are for
# the exact files from mesa-mesa-26.1.3.tar.gz, whose archive hash is pinned
# above.
verify_graphics_runtime_notice_file() {
  local artifact_label="$1"
  local expected="$2"
  local entry="$3"
  local actual="$4"
  if [[ "$actual" != "$expected" ]]; then
    printf '%s contains an unexpected Mesa notice SHA-256: %s\n' \
      "$artifact_label" "$entry" >&2
    exit 1
  fi
}

verify_portable_graphics_runtime_notice_file() {
  local expected="$1"
  local entry="$2"
  local actual
  actual="$(unzip -p "$WINDOWS_ARCHIVE" \
    "Ottrin-$VERSION-windows-x64/third-party-notices/mesa3d-26.1.3/$entry" | sha256sum | awk '{print $1}')"
  verify_graphics_runtime_notice_file 'Windows portable archive' "$expected" "$entry" "$actual"
}

verify_installer_graphics_runtime_notice_file() {
  local expected="$1"
  local entry="$2"
  local actual
  actual="$(7z x -so "$WINDOWS_INSTALLER" \
    "third-party-notices/mesa3d-26.1.3/$entry" | sha256sum | awk '{print $1}')"
  verify_graphics_runtime_notice_file 'Windows installer' "$expected" "$entry" "$actual"
}

for checksum_and_entry in \
  '0d1a0472ecc81830e75c20d59b0ea02841e3db21255e0ebad97ab682c54d6615 docs/license.rst' \
  '7e4d3fba250d15e9e0afa380d4b611828fa2560cc8fdebe95bcf2f01a487de12 licenses/Apache-2.0' \
  '9b9fea7520a816107fcac2598b7c0155222f16bc430e989706b22f4367f1df7d licenses/BSL-1.0' \
  '8ff76f1528e6e4e9400c0f6b1f2d19d4979aaac3bcc44bb707a55389132825c4 licenses/GPL-1.0-or-later' \
  '6667ad4063569cd53b13500550d93fcd3dc11bcf82c5514296af013a0b720ba5 licenses/GPL-2.0-only' \
  '323c587d0ccf10e376f8bf9a7f31fb4ca6078105194b42e0b1e0ee2bc9bde71f licenses/MIT' \
  '686bf035a1fd22076416fd3b90370ac67771e884bf57f55693d51f8ce7c710a7 licenses/SGI-B-2.0' \
  '9b4084655174654608fd037e9ddb4feeadfada01cd042e9c17b05aa7105b2b9f licenses/exceptions/Linux-Syscall-Note'; do
  read -r expected entry <<<"$checksum_and_entry"
  verify_portable_graphics_runtime_notice_file "$expected" "$entry"
  # The installer is verified later, after its NSIS payload has been opened.
done

verify_graphics_runtime_notice_manifest() {
  local artifact_label="$1"
  local manifest="$2"
  local expected
  for expected in \
    'Ottrin Windows graphics compatibility runtime' \
    "Mesa archive: $MESA_URL" \
    "Mesa archive SHA-256: $MESA_SHA256" \
    "Mesa source archive: $MESA_SOURCE_URL" \
    "Mesa source archive SHA-256: $MESA_SOURCE_SHA256" \
    "libEGL.dll SHA-256: $MESA_LIBEGL_SHA256" \
    "libGLESv2.dll SHA-256: $MESA_LIBGLESV2_SHA256" \
    "libgallium_wgl.dll SHA-256: $MESA_LIBGALLIUM_WGL_SHA256" \
    "pal1000 license: $PAL_LICENSE_URL" \
    "pal1000 license SHA-256: $PAL_LICENSE_SHA256" \
    "Microsoft DXC archive: $DXC_URL" \
    "Microsoft DXC archive SHA-256: $DXC_SHA256" \
    "dxil.dll SHA-256: $DXIL_SHA256" \
    'Included Mesa DLLs: libEGL.dll, libGLESv2.dll, libgallium_wgl.dll'; do
    if ! grep -Fqx "$expected" <<<"$manifest"; then
      printf '%s graphics-runtime notice manifest is missing: %s\\n' \
        "$artifact_label" "$expected" >&2
      exit 1
    fi
  done
}

# The compatibility DLLs are deliberately opt-in.  Verify that each shipped
# Windows artifact carries the exact recovery instructions, so a user who
# encounters a virtual or remote adapter failure can select the bundled path
# without changing the normal WGPU default.
verify_windows_graphics_recovery_instructions() {
  local artifact_label="$1"
  local instructions="$2"
  local expected
  for expected in \
    'ottrin.exe --renderer opengl' \
    'OTTRIN_RENDERER=opengl' \
    'WGPU with the system graphics driver'; do
    if ! grep -Fq "$expected" <<<"$instructions"; then
      printf '%s is missing the graphics recovery instruction: %s\n' \
        "$artifact_label" "$expected" >&2
      exit 1
    fi
  done
}

verify_portable_graphics_runtime_notice_manifest() {
  local manifest
  if ! manifest="$(unzip -p "$WINDOWS_ARCHIVE" \
    "Ottrin-$VERSION-windows-x64/third-party-notices/MANIFEST.txt")"; then
    printf 'Windows portable archive is missing the graphics-runtime notice manifest.\\n' >&2
    exit 1
  fi
  verify_graphics_runtime_notice_manifest 'Windows portable archive' "$manifest"
}

verify_portable_graphics_runtime_dll "$MESA_LIBEGL_SHA256" 'bin/libEGL.dll'
verify_portable_graphics_runtime_dll "$MESA_LIBGLESV2_SHA256" 'bin/libGLESv2.dll'
verify_portable_graphics_runtime_dll "$MESA_LIBGALLIUM_WGL_SHA256" 'bin/libgallium_wgl.dll'
verify_portable_graphics_runtime_dll "$DXIL_SHA256" 'bin/dxil.dll'
verify_portable_graphics_runtime_notice_manifest
if ! portable_running_instructions="$(unzip -p "$WINDOWS_ARCHIVE" \
  "Ottrin-$VERSION-windows-x64/RUNNING.md")"; then
  printf 'Windows portable archive is missing RUNNING.md.\n' >&2
  exit 1
fi
verify_windows_graphics_recovery_instructions \
  'Windows portable archive RUNNING.md' "$portable_running_instructions"

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
  'third-party-notices/mesa3d-26.1.3/licenses/Apache-2.0' \
  'third-party-notices/mesa3d-26.1.3/licenses/BSL-1.0' \
  'third-party-notices/mesa3d-26.1.3/licenses/GPL-1.0-or-later' \
  'third-party-notices/mesa3d-26.1.3/licenses/GPL-2.0-only' \
  'third-party-notices/mesa3d-26.1.3/licenses/MIT' \
  'third-party-notices/mesa3d-26.1.3/licenses/SGI-B-2.0' \
  'third-party-notices/mesa3d-26.1.3/licenses/exceptions/Linux-Syscall-Note' \
  'third-party-notices/pal1000-LICENSE.txt' \
  'third-party-notices/Microsoft-DXC-LICENSE-MS.txt' \
  'LICENSE' \
  'README.md' \
  'RUNNING.md' \
  'docs/USER_GUIDE.md' \
  'docs/DEVELOPMENT.md' \
  'docs/ROADMAP.md' \
  'docs/RELEASE_NOTES.md' \
  'docs/RELEASE_CHECKLIST.md'; do
  require_installer_entry "$entry"
done

for relative_path in \
  'LICENSE' \
  'README.md' \
  'docs/USER_GUIDE.md' \
  'docs/DEVELOPMENT.md' \
  'docs/ROADMAP.md' \
  'docs/RELEASE_NOTES.md' \
  'docs/RELEASE_CHECKLIST.md'; do
  if ! 7z x -so "$WINDOWS_INSTALLER" "$relative_path" | cmp -s - "$ROOT/$relative_path"; then
    printf 'Windows installer contains stale content: %s\n' "$relative_path" >&2
    exit 1
  fi
done

verify_installer_graphics_runtime_dll() {
  local expected="$1"
  local entry="$2"
  local actual
  actual="$(7z x -so "$WINDOWS_INSTALLER" "$entry" | sha256sum | awk '{print $1}')"
  if [[ "$actual" != "$expected" ]]; then
    printf 'Windows installer contains an unexpected graphics runtime DLL: %s\n' "$entry" >&2
    exit 1
  fi
}

verify_installer_graphics_runtime_notice_manifest() {
  local manifest
  if ! manifest="$(7z x -so "$WINDOWS_INSTALLER" third-party-notices/MANIFEST.txt)"; then
    printf 'Windows installer is missing the graphics-runtime notice manifest.\\n' >&2
    exit 1
  fi
  verify_graphics_runtime_notice_manifest 'Windows installer' "$manifest"
}

verify_installer_graphics_runtime_dll "$MESA_LIBEGL_SHA256" 'bin/libEGL.dll'
verify_installer_graphics_runtime_dll "$MESA_LIBGLESV2_SHA256" 'bin/libGLESv2.dll'
verify_installer_graphics_runtime_dll "$MESA_LIBGALLIUM_WGL_SHA256" 'bin/libgallium_wgl.dll'
verify_installer_graphics_runtime_dll "$DXIL_SHA256" 'bin/dxil.dll'
verify_installer_graphics_runtime_notice_manifest
for checksum_and_entry in \
  '0d1a0472ecc81830e75c20d59b0ea02841e3db21255e0ebad97ab682c54d6615 docs/license.rst' \
  '7e4d3fba250d15e9e0afa380d4b611828fa2560cc8fdebe95bcf2f01a487de12 licenses/Apache-2.0' \
  '9b9fea7520a816107fcac2598b7c0155222f16bc430e989706b22f4367f1df7d licenses/BSL-1.0' \
  '8ff76f1528e6e4e9400c0f6b1f2d19d4979aaac3bcc44bb707a55389132825c4 licenses/GPL-1.0-or-later' \
  '6667ad4063569cd53b13500550d93fcd3dc11bcf82c5514296af013a0b720ba5 licenses/GPL-2.0-only' \
  '323c587d0ccf10e376f8bf9a7f31fb4ca6078105194b42e0b1e0ee2bc9bde71f licenses/MIT' \
  '686bf035a1fd22076416fd3b90370ac67771e884bf57f55693d51f8ce7c710a7 licenses/SGI-B-2.0' \
  '9b4084655174654608fd037e9ddb4feeadfada01cd042e9c17b05aa7105b2b9f licenses/exceptions/Linux-Syscall-Note'; do
  read -r expected entry <<<"$checksum_and_entry"
  verify_installer_graphics_runtime_notice_file "$expected" "$entry"
done
if ! installer_running_instructions="$(7z x -so "$WINDOWS_INSTALLER" RUNNING.md)"; then
  printf 'Windows installer is missing RUNNING.md.\n' >&2
  exit 1
fi
verify_windows_graphics_recovery_instructions \
  'Windows installer RUNNING.md' "$installer_running_instructions"

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
