#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$ROOT/packaging/linux/bundle-runtime-libs.sh"
source "$ROOT/packaging/package-lifecycle.sh"
DIST_DIR="${1:-dist}"
VERSION="$(grep -m1 '^version = ' "$ROOT/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')"
EXPECTED_SOURCE_REVISION="$(git -C "$ROOT" rev-parse --verify HEAD)"
MESA_OPENGL32_SHA256="52119774eb912d07c3abfb6e3e894981f425b3c79d852f5363927e38fa3bde7e"
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
DXC_LICENSE_SHA256="734f72f239fe7b07b4c7203f294c1a7ce27095687278bab7e56d630d7c672963"

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
WINDOWS_MSI="$(find_one "Ottrin-$VERSION-windows-x64.msi" 'Windows MSI')"
LINUX_DEB="$(find_one "ottrin_${VERSION}_amd64.deb" 'Linux DEB')"
LINUX_RPM="$(find_one "ottrin-$VERSION-*.x86_64.rpm" 'Linux RPM')"
MACOS_ARCHIVE="$(find_one "Ottrin-$VERSION-macos-*.zip" 'macOS app archive')"

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
expect_artifact_name "$WINDOWS_MSI" "Ottrin-$VERSION-windows-x64.msi"

for artifact in "$LINUX_ARCHIVE" "$LINUX_APPIMAGE" "$WINDOWS_ARCHIVE" "$WINDOWS_INSTALLER" "$WINDOWS_MSI" "$MACOS_ARCHIVE" "$LINUX_DEB" "$LINUX_RPM"; do
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
MACOS_CONTENTS="$(unzip -Z1 "$MACOS_ARCHIVE")"

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

require_macos_zip_entry() {
  local pattern="$1"
  if ! grep -Eq "$pattern" <<<"$MACOS_CONTENTS"; then
    echo "macOS app archive is missing: $pattern" >&2
    exit 1
  fi
}

for entry in \
  '/bin/ottrin$' \
  '/bin/ottrin-priv-helper$' \
  '/libexec/ottrin/ottrin-legacy-doc-render$' \
  '/libexec/ottrin/ottrin-media-playback$' \
  '/libexec/ottrin/pdfium/lib/libpdfium\.so$' \
  '/libexec/ottrin/pdfium/LICENSE$' \
  '/libexec/ottrin/pdfium/licenses/pdfium\.txt$' \
  '/assets/icon\.svg$' \
  '/packaging/linux/org\.ottrin\.filesystem\.policy$' \
  '/LICENSE$' \
  '/SOURCE_REVISION\.txt$' \
  '/README\.md$' \
  '/RUNNING\.md$' \
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
  '/bin/ottrin-legacy-doc-render\.exe$' \
  '/bin/ottrin-priv-helper\.exe$' \
  '/bin/pdfium/pdfium\.dll$' \
  '/bin/pdfium/LICENSE$' \
  '/bin/pdfium/licenses/pdfium\.txt$' \
  '/bin/opengl32\.dll$' \
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
  '/SOURCE_REVISION\.txt$' \
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

for entry in \
  '^Ottrin\.app/Contents/Info\.plist$' \
  '^Ottrin\.app/Contents/MacOS/Ottrin$' \
  '^Ottrin\.app/Contents/MacOS/ottrin-indexd$' \
  '^Ottrin\.app/Contents/Resources/Ottrin\.png$' \
  '^Ottrin\.app/Contents/Resources/LICENSE$' \
  '^Ottrin\.app/Contents/Resources/README\.md$' \
  '^Ottrin\.app/Contents/Resources/SOURCE_REVISION\.txt$' \
  '^Ottrin\.app/Contents/Resources/docs/USER_GUIDE\.md$' \
  '^Ottrin\.app/Contents/Resources/docs/DEVELOPMENT\.md$' \
  '^Ottrin\.app/Contents/Resources/docs/ROADMAP\.md$' \
  '^Ottrin\.app/Contents/Resources/docs/RELEASE_NOTES\.md$' \
  '^Ottrin\.app/Contents/Resources/docs/RELEASE_CHECKLIST\.md$'; do
  require_macos_zip_entry "$entry"
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
  local macos_relative_path="$relative_path"
  if [[ "$relative_path" != LICENSE && "$relative_path" != README.md ]]; then
    macos_relative_path="docs/${relative_path#docs/}"
  fi
  if ! cmp -s \
    <(unzip -p "$MACOS_ARCHIVE" "Ottrin.app/Contents/Resources/$macos_relative_path") \
    "$ROOT/$relative_path"; then
    printf 'macOS app archive contains stale content: %s\n' "$relative_path" >&2
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

verify_source_revision() {
  local artifact_label="$1"
  local provenance="$2"
  for expected in \
    'Ottrin source revision' \
    "Version: $VERSION" \
    "Git revision: $EXPECTED_SOURCE_REVISION"; do
    if ! grep -Fqx "$expected" <<<"$provenance"; then
      printf '%s SOURCE_REVISION.txt is missing: %s\n' "$artifact_label" "$expected" >&2
      exit 1
    fi
  done
}

verify_source_revision \
  'Linux portable archive' \
  "$(tar -xOf "$LINUX_ARCHIVE" "Ottrin-$VERSION-linux-x86_64/SOURCE_REVISION.txt")"
verify_source_revision \
  'Windows portable archive' \
  "$(unzip -p "$WINDOWS_ARCHIVE" "Ottrin-$VERSION-windows-x64/SOURCE_REVISION.txt")"
verify_source_revision \
  'macOS app archive' \
  "$(unzip -p "$MACOS_ARCHIVE" 'Ottrin.app/Contents/Resources/SOURCE_REVISION.txt')"

# RUNNING.md is the artifact-local setup guidance for the portable Linux
# release.  Require its safety-critical helper instructions independently of
# the archive checksum so a substituted payload cannot silently lose them.
verify_linux_portable_running_instructions() {
  local instructions="$1"
  local expected
  for expected in \
    'sudo install -D -m 0755 ./bin/ottrin-priv-helper /usr/libexec/ottrin/ottrin-priv-helper' \
    'Without the helper, normal user-owned file operations still work.'; do
    if ! grep -Fq "$expected" <<<"$instructions"; then
      printf 'Linux portable archive RUNNING.md is missing the runtime instruction: %s\n' \
        "$expected" >&2
      exit 1
    fi
  done
}

if ! linux_portable_running_instructions="$(tar -xOf "$LINUX_ARCHIVE" \
  "Ottrin-$VERSION-linux-x86_64/RUNNING.md")"; then
  printf 'Linux portable archive is missing RUNNING.md.\n' >&2
  exit 1
fi
verify_linux_portable_running_instructions "$linux_portable_running_instructions"

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

verify_portable_graphics_runtime_notice_root_file() {
  local expected="$1"
  local entry="$2"
  local actual
  actual="$(unzip -p "$WINDOWS_ARCHIVE" \
    "Ottrin-$VERSION-windows-x64/third-party-notices/$entry" | sha256sum | awk '{print $1}')"
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

verify_installer_graphics_runtime_notice_root_file() {
  local expected="$1"
  local entry="$2"
  local actual
  actual="$(7z x -so "$WINDOWS_INSTALLER" \
    "third-party-notices/$entry" | sha256sum | awk '{print $1}')"
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
verify_portable_graphics_runtime_notice_root_file "$PAL_LICENSE_SHA256" 'pal1000-LICENSE.txt'
verify_portable_graphics_runtime_notice_root_file "$DXC_LICENSE_SHA256" 'Microsoft-DXC-LICENSE-MS.txt'

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
    "opengl32.dll SHA-256: $MESA_OPENGL32_SHA256" \
    "libEGL.dll SHA-256: $MESA_LIBEGL_SHA256" \
    "libGLESv2.dll SHA-256: $MESA_LIBGLESV2_SHA256" \
    "libgallium_wgl.dll SHA-256: $MESA_LIBGALLIUM_WGL_SHA256" \
    "pal1000 license: $PAL_LICENSE_URL" \
    "pal1000 license SHA-256: $PAL_LICENSE_SHA256" \
    "Microsoft DXC archive: $DXC_URL" \
    "Microsoft DXC archive SHA-256: $DXC_SHA256" \
    "Microsoft DXC license SHA-256: $DXC_LICENSE_SHA256" \
    "dxil.dll SHA-256: $DXIL_SHA256" \
    'Included Mesa DLLs: opengl32.dll, libEGL.dll, libGLESv2.dll, libgallium_wgl.dll'; do
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

# The compatibility runtime is not the only safety-critical Windows guidance:
# the same artifact-local instructions must retain the UAC/helper boundary and
# make clear that ordinary user-owned operations never require elevation.
verify_windows_runtime_instructions() {
  local artifact_label="$1"
  local instructions="$2"
  local expected
  for expected in \
    '.\bin\ottrin-priv-helper.exe' \
    'Normal user-owned file operations do not require elevation.' \
    'Denied operations use the bundled helper through the Windows UAC flow.'; do
    if ! grep -Fq "$expected" <<<"$instructions"; then
      printf '%s is missing the Windows runtime instruction: %s\\n' \
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

verify_portable_graphics_runtime_dll "$MESA_OPENGL32_SHA256" 'bin/opengl32.dll'
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
verify_windows_runtime_instructions \
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
  'bin/ottrin-priv-helper.exe' \
  'bin/pdfium/pdfium.dll' \
  'bin/opengl32.dll' \
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
  'SOURCE_REVISION.txt' \
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

verify_source_revision \
  'Windows installer' \
  "$(7z x -so "$WINDOWS_INSTALLER" SOURCE_REVISION.txt)"

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

verify_installer_graphics_runtime_dll "$MESA_OPENGL32_SHA256" 'bin/opengl32.dll'
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
verify_installer_graphics_runtime_notice_root_file "$PAL_LICENSE_SHA256" 'pal1000-LICENSE.txt'
verify_installer_graphics_runtime_notice_root_file "$DXC_LICENSE_SHA256" 'Microsoft-DXC-LICENSE-MS.txt'
if ! installer_running_instructions="$(7z x -so "$WINDOWS_INSTALLER" RUNNING.md)"; then
  printf 'Windows installer is missing RUNNING.md.\n' >&2
  exit 1
fi
verify_windows_graphics_recovery_instructions \
  'Windows installer RUNNING.md' "$installer_running_instructions"
verify_windows_runtime_instructions \
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

PORTABLE_EXTRACT_DIR="$APPIMAGE_EXTRACT_DIR/linux-portable"
mkdir -p "$PORTABLE_EXTRACT_DIR"
tar -xzf "$LINUX_ARCHIVE" -C "$PORTABLE_EXTRACT_DIR"
PORTABLE_STAGE="$PORTABLE_EXTRACT_DIR/Ottrin-$VERSION-linux-x86_64"
verify_linux_runtime_closure \
  'Linux portable archive' \
  "$PORTABLE_STAGE" \
  "$PORTABLE_STAGE/libexec/ottrin/ottrin" \
  "$PORTABLE_STAGE/lib" \
  "$PORTABLE_STAGE/third-party-notices/linux-runtime/BUNDLED_RUNTIME_MANIFEST.txt" \
  "$PORTABLE_STAGE/third-party-notices/linux-runtime" \
  true
verify_linux_runtime_closure \
  'Linux portable native DOC helper' \
  "$PORTABLE_STAGE" \
  "$PORTABLE_STAGE/libexec/ottrin/ottrin-legacy-doc-render" \
  "$PORTABLE_STAGE/lib" \
  "$PORTABLE_STAGE/third-party-notices/linux-runtime/BUNDLED_RUNTIME_MANIFEST.txt" \
  "$PORTABLE_STAGE/third-party-notices/linux-runtime" \
  false
verify_linux_media_runtime_boundary \
  'Linux portable archive' \
  "$PORTABLE_STAGE" \
  "$PORTABLE_STAGE/libexec/ottrin/ottrin-media-playback" \
  "$PORTABLE_STAGE/lib"
verify_linux_runtime_closure \
  'Linux AppImage' \
  "$APPIMAGE_EXTRACT_DIR/squashfs-root" \
  "$APPIMAGE_EXTRACT_DIR/squashfs-root/usr/bin/ottrin" \
  "$APPIMAGE_EXTRACT_DIR/squashfs-root/usr/lib" \
  "$APPIMAGE_EXTRACT_DIR/squashfs-root/usr/share/doc/ottrin/third-party-notices/linux-runtime/BUNDLED_RUNTIME_MANIFEST.txt" \
  "$APPIMAGE_EXTRACT_DIR/squashfs-root/usr/share/doc/ottrin/third-party-notices/linux-runtime" \
  true
verify_linux_runtime_closure \
  'Linux AppImage native DOC helper' \
  "$APPIMAGE_EXTRACT_DIR/squashfs-root" \
  "$APPIMAGE_EXTRACT_DIR/squashfs-root/usr/bin/ottrin-legacy-doc-render" \
  "$APPIMAGE_EXTRACT_DIR/squashfs-root/usr/lib" \
  "$APPIMAGE_EXTRACT_DIR/squashfs-root/usr/share/doc/ottrin/third-party-notices/linux-runtime/BUNDLED_RUNTIME_MANIFEST.txt" \
  "$APPIMAGE_EXTRACT_DIR/squashfs-root/usr/share/doc/ottrin/third-party-notices/linux-runtime" \
  false
verify_linux_media_runtime_boundary \
  'Linux AppImage' \
  "$APPIMAGE_EXTRACT_DIR/squashfs-root" \
  "$APPIMAGE_EXTRACT_DIR/squashfs-root/usr/bin/ottrin-media-playback" \
  "$APPIMAGE_EXTRACT_DIR/squashfs-root/usr/lib"

for entry in \
  'AppRun' \
  'usr/bin/ottrin' \
  'usr/bin/ottrin-legacy-doc-render' \
  'usr/bin/ottrin-media-playback' \
  'usr/libexec/ottrin/ottrin-priv-helper' \
  'usr/bin/pdfium/lib/libpdfium.so' \
  'usr/bin/pdfium/LICENSE' \
  'usr/bin/pdfium/licenses/pdfium.txt' \
  'usr/share/applications/org.ottrin.Ottrin.desktop' \
  'usr/share/metainfo/org.ottrin.Ottrin.appdata.xml' \
  'usr/share/polkit-1/actions/org.ottrin.filesystem.policy' \
  'usr/share/doc/ottrin/README.md' \
  'usr/share/doc/ottrin/LICENSE' \
  'usr/share/doc/ottrin/SOURCE_REVISION.txt' \
  'usr/share/doc/ottrin/USER_GUIDE.md' \
  'usr/share/doc/ottrin/DEVELOPMENT.md' \
  'usr/share/doc/ottrin/ROADMAP.md' \
  'usr/share/doc/ottrin/RELEASE_NOTES.md' \
  'usr/share/doc/ottrin/RELEASE_CHECKLIST.md'; do
  if [[ ! -e "$APPIMAGE_EXTRACT_DIR/squashfs-root/$entry" ]]; then
    printf 'Linux AppImage is missing: %s\n' "$entry" >&2
    exit 1
  fi
done

# The privileged helper policy controls an authorization boundary. Presence of
# its pathname is insufficient: release artifacts must carry the exact,
# reviewed policy bytes from this checkout.
verify_linux_privileged_helper_policy_bytes() {
  local artifact_label="$1"
  local artifact_policy="$2"
  if ! cmp -s "$artifact_policy" \
    "$ROOT/packaging/linux/org.ottrin.filesystem.policy"; then
    printf '%s contains stale privileged-helper policy.\n' "$artifact_label" >&2
    exit 1
  fi
}

PORTABLE_POLICY="$APPIMAGE_EXTRACT_DIR/linux-portable-policy"
if ! tar -xOf "$LINUX_ARCHIVE" \
  "Ottrin-$VERSION-linux-x86_64/packaging/linux/org.ottrin.filesystem.policy" \
  > "$PORTABLE_POLICY"; then
  printf 'Linux portable archive is missing privileged-helper policy.\n' >&2
  exit 1
fi
verify_linux_privileged_helper_policy_bytes \
  'Linux portable archive' "$PORTABLE_POLICY"
verify_linux_privileged_helper_policy_bytes \
  'Linux AppImage' \
  "$APPIMAGE_EXTRACT_DIR/squashfs-root/usr/share/polkit-1/actions/org.ottrin.filesystem.policy"

# Native packages are release deliverables too. Inspect actual payloads and
# generated dependencies; checksums alone do not establish usable installation.
for tool in dpkg-deb rpm rpm2cpio cpio objdump objcopy; do
  command -v "$tool" >/dev/null || {
    printf '%s is required to inspect native Linux packages.\n' "$tool" >&2
    exit 1
  }
done
DEB_STAGE="$APPIMAGE_EXTRACT_DIR/deb"
RPM_STAGE="$APPIMAGE_EXTRACT_DIR/rpm"
mkdir -p "$DEB_STAGE" "$RPM_STAGE"
dpkg-deb -x "$LINUX_DEB" "$DEB_STAGE"
rpm2cpio "$LINUX_RPM" | (cd "$RPM_STAGE"; cpio -idm --quiet)
for stage in "$DEB_STAGE" "$RPM_STAGE"; do
  for executable in usr/bin/ottrin usr/bin/ottrin-indexd \
    usr/bin/ottrin-media-playback usr/bin/ottrin-legacy-doc-render \
    usr/libexec/ottrin/ottrin-priv-helper; do
    test -x "$stage/$executable" || {
      printf 'Native package is missing executable: %s\n' "$executable" >&2
      exit 1
    }
  done
  # DEB preserves executable bytes; RPM performs standard symbol stripping.
  # Compare actual release code as well as the separately staged source note.
  for executable in usr/bin/ottrin usr/bin/ottrin-indexd \
    usr/bin/ottrin-media-playback usr/bin/ottrin-legacy-doc-render \
    usr/libexec/ottrin/ottrin-priv-helper; do
    expected="$APPIMAGE_EXTRACT_DIR/squashfs-root/$executable"
    if [[ "$stage" == "$DEB_STAGE" ]]; then
      cmp "$stage/$executable" "$expected"
    else
      for section in .text .rodata; do
        objcopy --dump-section "$section=$APPIMAGE_EXTRACT_DIR/native-section" \
          "$stage/$executable" "$APPIMAGE_EXTRACT_DIR/native-dump"
        objcopy --dump-section "$section=$APPIMAGE_EXTRACT_DIR/appimage-section" \
          "$expected" "$APPIMAGE_EXTRACT_DIR/appimage-dump"
        cmp "$APPIMAGE_EXTRACT_DIR/native-section" "$APPIMAGE_EXTRACT_DIR/appimage-section"
      done
    fi
  done
  verify_source_revision 'Linux native package' \
    "$(<"$stage/usr/share/doc/ottrin/SOURCE_REVISION.txt")"
  verify_linux_privileged_helper_policy_bytes 'Linux native package' \
    "$stage/usr/share/polkit-1/actions/org.ottrin.filesystem.policy"
done
DEB_LIBC_MIN="$(ottrin_native_glibc_requirement "$DEB_STAGE")"
DEB_DEPENDS="$(dpkg-deb -f "$LINUX_DEB" Depends)"
for dependency in "libc6 (>= $DEB_LIBC_MIN)" pkexec polkitd; do
  if [[ ", $DEB_DEPENDS," != *", $dependency,"* ]]; then
    printf 'Linux DEB is missing runtime dependency: %s\n' "$dependency" >&2
    exit 1
  fi
done
RPM_LIBC_MIN="$(ottrin_native_glibc_requirement "$RPM_STAGE")"
RPM_DEPENDS="$(rpm -qp --queryformat '[%{REQUIRENAME}\n]' "$LINUX_RPM")"
for dependency in "libc.so.6(GLIBC_$RPM_LIBC_MIN)(64bit)" polkit /usr/bin/pkexec; do
  if ! grep -Fxq "$dependency" <<<"$RPM_DEPENDS"; then
    printf 'Linux RPM is missing runtime dependency: %s\n' "$dependency" >&2
    exit 1
  fi
done

verify_source_revision \
  'Linux AppImage' \
  "$(<"$APPIMAGE_EXTRACT_DIR/squashfs-root/usr/share/doc/ottrin/SOURCE_REVISION.txt")"

# The AppImage is a separately assembled release artifact.  Its provenance
# note identifies the build commit, but does not prove that every public
# document still matches the checkout being released.  Compare those bytes
# directly, as is already required for the portable archives and installer.
verify_appimage_release_file_bytes() {
  local relative_path="$1"
  local source_path="$relative_path"
  case "$relative_path" in
    USER_GUIDE.md|DEVELOPMENT.md|ROADMAP.md|RELEASE_NOTES.md|RELEASE_CHECKLIST.md)
      source_path="docs/$relative_path"
      ;;
  esac
  if ! cmp -s \
    "$APPIMAGE_EXTRACT_DIR/squashfs-root/usr/share/doc/ottrin/$relative_path" \
    "$ROOT/$source_path"; then
    printf 'Linux AppImage contains stale content: %s\n' "$relative_path" >&2
    exit 1
  fi
}

verify_appimage_release_file_bytes 'README.md'
verify_appimage_release_file_bytes 'LICENSE'
verify_appimage_release_file_bytes 'USER_GUIDE.md'
verify_appimage_release_file_bytes 'DEVELOPMENT.md'
verify_appimage_release_file_bytes 'ROADMAP.md'
verify_appimage_release_file_bytes 'RELEASE_NOTES.md'
verify_appimage_release_file_bytes 'RELEASE_CHECKLIST.md'

require_zip_entry '/bin/pdfium/pdfium\.dll$'

printf 'Verified release artifacts in %s\n' "$DIST_DIR"
