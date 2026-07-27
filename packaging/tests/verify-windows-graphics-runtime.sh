#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BASH_PACKAGER="$ROOT/packaging/windows/package-portable.sh"
POWERSHELL_PACKAGER="$ROOT/packaging/windows/package-portable.ps1"
ARTIFACT_VERIFIER="$ROOT/packaging/verify-release-artifacts.sh"

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
  'sha256sum -c -' \
  'checksum mismatch after extracting the pinned Mesa archive'; do
  require_literal "$BASH_PACKAGER" "$literal"
done

for literal in \
  '$MesaLibEglSha256 = "ce36d60c5602920df49a2c8939d903da05dfe2610bfbb4bf0676e9ec3453df3b"' \
  '$MesaLibGlesv2Sha256 = "649f779b1b945433dfc2740e9ff1366e309686043999f7271e550079eadde8e6"' \
  '$MesaLibGalliumWglSha256 = "a3917a52cc893f614c9e415f5442eb2981dd46ba636fc64b35d5bd4be0faec7b"' \
  'Mesa license extraction from the pinned source archive failed.' \
  'finally {' \
  'Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue' \
  'Mesa DLL checksum mismatch after extracting the pinned Mesa archive'; do
  require_literal "$POWERSHELL_PACKAGER" "$literal"
done

# The final artifact verifier must independently pin DXIL too: a release
# checksum only authenticates whatever is presented alongside it.
for literal in \
  'DXIL_SHA256="cbcfe883a09fd0ca1f98abdf3a9553b560895e3283a136da82a8381253a169df"' \
  'verify_portable_graphics_runtime_notice_manifest' \
  'verify_installer_graphics_runtime_notice_manifest' \
  'verify_portable_graphics_runtime_dll "$DXIL_SHA256" '\''bin/dxil.dll'\''' \
  'verify_installer_graphics_runtime_dll "$DXIL_SHA256" '\''bin/dxil.dll'\'''; do
  require_literal "$ARTIFACT_VERIFIER" "$literal"
done
