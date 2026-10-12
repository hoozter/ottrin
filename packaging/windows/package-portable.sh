#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$ROOT/packaging/package-lifecycle.sh"
VERSION="${OTTRIN_VERSION:-$(grep -m1 '^version = ' "$ROOT/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')}"
TARGET="${OTTRIN_TARGET:-x86_64-pc-windows-gnu}"
BUILD_DIR="$ROOT/target/$TARGET/release"
DIST_DIR="${OTTRIN_DIST_DIR:-$ROOT/dist}"
STAGE_ROOT="$DIST_DIR/windows-payload"
STAGE="$STAGE_ROOT/Ottrin-$VERSION-windows-x64"
ARCHIVE="$DIST_DIR/Ottrin-$VERSION-windows-x64-portable.zip"
PDFIUM_URL="https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/7961/pdfium-win-x64.tgz"
PDFIUM_SHA256="88276459349b291c41f10422dad0210f007c04d919c8fa56472b6b7c6406adf4"
MESA_URL="https://github.com/pal1000/mesa-dist-win/releases/download/26.1.3/mesa3d-26.1.3-release-mingw.7z"
MESA_SHA256="80d5add64254c839b4c784bdab6a2b504e448675604b0fe54a9bce3c543303a7"
MESA_SOURCE_URL="https://gitlab.freedesktop.org/mesa/mesa/-/archive/mesa-26.1.3/mesa-mesa-26.1.3.tar.gz"
MESA_SOURCE_SHA256="4ef7c87feb15966d5645ec55728020632d2f54becb42ce546d4a12bfc5275dd1"
MESA_OPENGL32_SHA256="52119774eb912d07c3abfb6e3e894981f425b3c79d852f5363927e38fa3bde7e"
MESA_LIBEGL_SHA256="ce36d60c5602920df49a2c8939d903da05dfe2610bfbb4bf0676e9ec3453df3b"
MESA_LIBGLESV2_SHA256="649f779b1b945433dfc2740e9ff1366e309686043999f7271e550079eadde8e6"
MESA_LIBGALLIUM_WGL_SHA256="a3917a52cc893f614c9e415f5442eb2981dd46ba636fc64b35d5bd4be0faec7b"
PAL_LICENSE_URL="https://raw.githubusercontent.com/pal1000/mesa-dist-win/26.1.3/LICENSE"
PAL_LICENSE_SHA256="9cd0121dc070f0ac65c1fc266bee74bdbbc22cea4210ae72ae0c1f076448d4cc"
DXC_URL="https://github.com/microsoft/DirectXShaderCompiler/releases/download/v1.9.2602.24/dxc_2026_05_27.zip"
DXC_SHA256="cf658aacf070d3045e31b8f1f8a696c2945f37c1095019481ef7c513368db3b4"
DXC_LICENSE_SHA256="734f72f239fe7b07b4c7203f294c1a7ce27095687278bab7e56d630d7c672963"
DXIL_SHA256="cbcfe883a09fd0ca1f98abdf3a9553b560895e3283a136da82a8381253a169df"


download_verified() {
    local url="$1"
    local expected="$2"
    local destination="$3"
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$url" -o "$destination"
    else
        wget -q "$url" -O "$destination"
    fi
    printf '%s  %s\n' "$expected" "$destination" | sha256sum -c -
}

download_verify_and_package_pdfium() (
    local target_pdfium_dir="$1"
    local tmp_dir
    tmp_dir="$(mktemp -d)"
    trap 'rm -rf "$tmp_dir"' EXIT
    local archive="$tmp_dir/pdfium.tgz"
    local extract_dir="$tmp_dir/extract"
    mkdir -p "$extract_dir"

    download_verified "$PDFIUM_URL" "$PDFIUM_SHA256" "$archive"
    tar -xzf "$archive" -C "$extract_dir"

    mkdir -p "$target_pdfium_dir"
    cp -a "$extract_dir/bin/." "$target_pdfium_dir/"
    cp -a "$extract_dir/LICENSE" "$extract_dir/licenses" "$target_pdfium_dir/"
)

package_graphics_compatibility_runtime() (
    local target_bin="$1"
    local target_notices="$2"
    local tmp_dir
    tmp_dir="$(mktemp -d)"
    trap 'rm -rf "$tmp_dir"' EXIT
    local mesa_archive="$tmp_dir/mesa.7z"
    local mesa_source="$tmp_dir/mesa-source.tar.gz"
    local dxc_archive="$tmp_dir/dxc.zip"

    download_verified "$MESA_URL" "$MESA_SHA256" "$mesa_archive"
    download_verified "$MESA_SOURCE_URL" "$MESA_SOURCE_SHA256" "$mesa_source"
    download_verified "$PAL_LICENSE_URL" "$PAL_LICENSE_SHA256" "$tmp_dir/pal1000-LICENSE.txt"
    download_verified "$DXC_URL" "$DXC_SHA256" "$dxc_archive"

    mkdir -p "$tmp_dir/dxc" "$target_notices/mesa3d-26.1.3"
    for dll in opengl32.dll libEGL.dll libGLESv2.dll libgallium_wgl.dll; do
        7z e -bd -y "-o$target_bin" "$mesa_archive" "x64/$dll" >/dev/null
    done
    for checksum_and_dll in \
        "$MESA_OPENGL32_SHA256 opengl32.dll" \
        "$MESA_LIBEGL_SHA256 libEGL.dll" \
        "$MESA_LIBGLESV2_SHA256 libGLESv2.dll" \
        "$MESA_LIBGALLIUM_WGL_SHA256 libgallium_wgl.dll"; do
        read -r expected dll <<<"$checksum_and_dll"
        if ! printf '%s  %s\n' "$expected" "$target_bin/$dll" | sha256sum -c -; then
            printf 'Mesa DLL checksum mismatch after extracting the pinned Mesa archive: %s\n' "$dll" >&2
            exit 1
        fi
    done
    unzip -q "$dxc_archive" -d "$tmp_dir/dxc"
    printf '%s  %s\n' "$DXC_LICENSE_SHA256" "$tmp_dir/dxc/LICENSE-MS.txt" | sha256sum -c -
    printf '%s  %s\n' "$DXIL_SHA256" "$tmp_dir/dxc/bin/x64/dxil.dll" | sha256sum -c -
    cp "$tmp_dir/dxc/bin/x64/dxil.dll" "$target_bin/dxil.dll"

    tar -xzf "$mesa_source" -C "$target_notices/mesa3d-26.1.3" --strip-components=1 \
        mesa-mesa-26.1.3/docs/license.rst mesa-mesa-26.1.3/licenses
    cp "$tmp_dir/pal1000-LICENSE.txt" "$target_notices/pal1000-LICENSE.txt"
    cp "$tmp_dir/dxc/LICENSE-MS.txt" "$target_notices/Microsoft-DXC-LICENSE-MS.txt"
    cat > "$target_notices/MANIFEST.txt" <<EOF
Ottrin Windows graphics compatibility runtime

Mesa archive: $MESA_URL
Mesa archive SHA-256: $MESA_SHA256
Mesa source archive: $MESA_SOURCE_URL
Mesa source archive SHA-256: $MESA_SOURCE_SHA256
opengl32.dll SHA-256: $MESA_OPENGL32_SHA256
libEGL.dll SHA-256: $MESA_LIBEGL_SHA256
libGLESv2.dll SHA-256: $MESA_LIBGLESV2_SHA256
libgallium_wgl.dll SHA-256: $MESA_LIBGALLIUM_WGL_SHA256
pal1000 license: $PAL_LICENSE_URL
pal1000 license SHA-256: $PAL_LICENSE_SHA256
Microsoft DXC archive: $DXC_URL
Microsoft DXC archive SHA-256: $DXC_SHA256
Microsoft DXC license SHA-256: $DXC_LICENSE_SHA256
dxil.dll SHA-256: $DXIL_SHA256
Included Mesa DLLs: opengl32.dll, libEGL.dll, libGLESv2.dll, libgallium_wgl.dll
EOF
)

cd "$ROOT"
cargo build --release --target "$TARGET" -p ottrin-app -p ottrin-platform -p ottrin-search
cargo build --release --target "$TARGET" -p ottrin-preview --features windows-office --bin ottrin-legacy-doc-render --bin ottrin-office-render
"$ROOT/scripts/windows-gui-subsystem-smoke.sh" "$BUILD_DIR/ottrin-app.exe"

mkdir -p "$DIST_DIR"
WORK="$(mktemp -d "$DIST_DIR/.windows-portable.XXXXXX")"
WORK="$(cd "$WORK" && pwd)"
trap 'rm -rf "$WORK"' EXIT
rm -rf "$STAGE_ROOT"
mkdir -p "$STAGE/bin" "$STAGE/assets" "$STAGE/docs" "$STAGE/third-party-notices"

cp "$BUILD_DIR/ottrin-app.exe" "$STAGE/bin/ottrin.exe"
cp "$BUILD_DIR/ottrin-office-render.exe" "$STAGE/bin/ottrin-office-render.exe"
cp "$BUILD_DIR/ottrin-legacy-doc-render.exe" "$STAGE/bin/ottrin-legacy-doc-render.exe"
cp "$BUILD_DIR/ottrin-priv-helper.exe" "$STAGE/bin/ottrin-priv-helper.exe"
download_verify_and_package_pdfium "$STAGE/bin/pdfium"
package_graphics_compatibility_runtime "$STAGE/bin" "$STAGE/third-party-notices"
cp "$ROOT/assets/icon.svg" "$ROOT/assets/icon.png" "$ROOT/assets/icon.ico" "$ROOT/assets/logo.svg" "$STAGE/assets/"
cp "$ROOT/README.md" "$STAGE/"
cp "$ROOT/LICENSE" "$STAGE/"
cp "$ROOT/docs/USER_GUIDE.md" "$ROOT/docs/DEVELOPMENT.md" "$ROOT/docs/ROADMAP.md" \
  "$ROOT/docs/RELEASE_NOTES.md" "$ROOT/docs/RELEASE_CHECKLIST.md" "$STAGE/docs/"
ottrin_write_source_revision "$ROOT" "$STAGE/SOURCE_REVISION.txt" "$VERSION"

cat > "$STAGE/RUNNING.md" <<'EOF'
# Running Ottrin Portable on Windows

Run:

```powershell
.\bin\ottrin.exe
```

The privileged helper is bundled next to the app as:

```text
.\bin\ottrin-priv-helper.exe
```

Normal user-owned file operations do not require elevation. Denied operations use the bundled helper through the Windows UAC flow.

On virtual, remote, or problematic graphics adapters, keep the bundled Mesa
compatibility runtime and explicitly select it for this launch:

```powershell
.\bin\ottrin.exe --renderer opengl
```

For a user-level recovery override, set `OTTRIN_RENDERER=opengl` before launching
Ottrin. The normal default remains WGPU with the system graphics driver.
EOF

(cd "$STAGE_ROOT" && zip -qr "$WORK/$(basename "$ARCHIVE")" "$(basename "$STAGE")")
ottrin_publish_artifact "$WORK/$(basename "$ARCHIVE")" "$ARCHIVE"
sha256sum "$ARCHIVE" | sed "s|$ARCHIVE|$(basename "$ARCHIVE")|" > "$ARCHIVE.sha256"

printf '%s\n' "$ARCHIVE"
