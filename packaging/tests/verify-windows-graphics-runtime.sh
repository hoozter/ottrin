#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BASH_PACKAGER="$ROOT/packaging/windows/package-portable.sh"
POWERSHELL_PACKAGER="$ROOT/packaging/windows/package-portable.ps1"
ARTIFACT_VERIFIER="$ROOT/packaging/verify-release-artifacts.sh"
INSTALLER_PACKAGER="$ROOT/packaging/windows/package-installer.sh"
POWERSHELL_INSTALLER_PACKAGER="$ROOT/packaging/windows/package-installer.ps1"

require_literal() {
  local file="$1"
  local literal="$2"
  if ! grep -Fq -- "$literal" "$file"; then
    printf 'missing graphics-runtime integrity guard in %s: %s\n' "$file" "$literal" >&2
    exit 1
  fi
}

# These are hashes of the exact x64 DLLs extracted from the pinned
# pal1000 Mesa 26.1.3 MinGW archive.  Both packagers must verify the
# post-extraction bytes, not only the enclosing archive.
for literal in \
  'MESA_LIBEGL_SHA256="ce36d60c5602920df49a2c8939d903da05dfe2610bfbb4bf0676e9ec3453df3b"' \
  'MESA_LIBGLESV2_SHA256="649f779b1b945433dfc2740e9ff1366e309686043999f7271e550079eadde8e6"' \
  'MESA_LIBGALLIUM_WGL_SHA256="a3917a52cc893f614c9e415f5442eb2981dd46ba636fc64b35d5bd4be0faec7b"' \
  'trap ' \
  'rm -rf "$tmp_dir"' \
  'sha256sum -c -' \
  'ottrin.exe --renderer opengl' \
  'OTTRIN_RENDERER=opengl' \
  'checksum mismatch after extracting the pinned Mesa archive'; do
  require_literal "$BASH_PACKAGER" "$literal"
done

for literal in \
  '$MesaLibEglSha256 = "ce36d60c5602920df49a2c8939d903da05dfe2610bfbb4bf0676e9ec3453df3b"' \
  '$MesaLibGlesv2Sha256 = "649f779b1b945433dfc2740e9ff1366e309686043999f7271e550079eadde8e6"' \
  '$MesaLibGalliumWglSha256 = "a3917a52cc893f614c9e415f5442eb2981dd46ba636fc64b35d5bd4be0faec7b"' \
  '$mesaNotices = Join-Path $NoticeDestination "mesa3d-26.1.3"' \
  '-C $mesaNotices --strip-components=1' \
  'Mesa license extraction from the pinned source archive failed.' \
  'finally {' \
  'Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue' \
  'ottrin.exe --renderer opengl' \
  'OTTRIN_RENDERER=opengl' \
  'Mesa DLL checksum mismatch after extracting the pinned Mesa archive'; do
  require_literal "$POWERSHELL_PACKAGER" "$literal"
done

# The final artifact verifier must independently pin DXIL too: a release
# checksum only authenticates whatever is presented alongside it.
for literal in \
  'DXIL_SHA256="cbcfe883a09fd0ca1f98abdf3a9553b560895e3283a136da82a8381253a169df"' \
  'third-party-notices/mesa3d-26\.1\.3/licenses/Apache-2\.0' \
  'third-party-notices/mesa3d-26\.1\.3/licenses/BSL-1\.0' \
  'third-party-notices/mesa3d-26\.1\.3/licenses/GPL-1\.0-or-later' \
  'third-party-notices/mesa3d-26\.1\.3/licenses/GPL-2\.0-only' \
  'third-party-notices/mesa3d-26\.1\.3/licenses/SGI-B-2\.0' \
  'third-party-notices/mesa3d-26\.1\.3/licenses/exceptions/Linux-Syscall-Note' \
  'verify_portable_graphics_runtime_notice_manifest' \
  'verify_installer_graphics_runtime_notice_manifest' \
  'verify_windows_graphics_recovery_instructions' \
  'verify_portable_graphics_runtime_dll "$DXIL_SHA256" '\''bin/dxil.dll'\''' \
  'verify_installer_graphics_runtime_dll "$DXIL_SHA256" '\''bin/dxil.dll'\'''; do
  require_literal "$ARTIFACT_VERIFIER" "$literal"
done

# The installer copies its supplied stage verbatim.  It must therefore reject
# a stage missing any notice that the final artifact verifier requires, rather
# than relying on a later archive inspection to catch an incomplete payload.
# The same applies to RUNNING.md: it is the only packaged, artifact-local
# recovery instruction for choosing the explicitly bundled OpenGL path.
for literal in \
  '  LICENSE' \
  'RUNNING.md' \
  'third-party-notices/mesa3d-26.1.3/licenses/Apache-2.0' \
  'third-party-notices/mesa3d-26.1.3/licenses/BSL-1.0' \
  'third-party-notices/mesa3d-26.1.3/licenses/GPL-1.0-or-later' \
  'third-party-notices/mesa3d-26.1.3/licenses/GPL-2.0-only' \
  'third-party-notices/mesa3d-26.1.3/licenses/SGI-B-2.0' \
  'third-party-notices/mesa3d-26.1.3/licenses/exceptions/Linux-Syscall-Note'; do
  require_literal "$INSTALLER_PACKAGER" "$literal"
done

for literal in \
  '    "LICENSE",' \
  'RUNNING.md' \
  'third-party-notices\mesa3d-26.1.3\licenses\Apache-2.0' \
  'third-party-notices\mesa3d-26.1.3\licenses\BSL-1.0' \
  'third-party-notices\mesa3d-26.1.3\licenses\GPL-1.0-or-later' \
  'third-party-notices\mesa3d-26.1.3\licenses\GPL-2.0-only' \
  'third-party-notices\mesa3d-26.1.3\licenses\SGI-B-2.0' \
  'third-party-notices\mesa3d-26.1.3\licenses\exceptions\Linux-Syscall-Note'; do
  require_literal "$POWERSHELL_INSTALLER_PACKAGER" "$literal"
done
