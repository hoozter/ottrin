#!/usr/bin/env bash
# Pinned PDF renderer and license payload shared by every Linux package.
PDFIUM_URL="https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/7961/pdfium-linux-x64.tgz"
PDFIUM_SHA256="019665c8877d46fe65f625f80fd714ab07aac68554b0636acf2a2adf9288adb2"


package_linux_pdfium() (
    local target_pdfium_dir="$1"
    local tmp_dir
    tmp_dir="$(mktemp -d)"
    trap 'rm -rf "$tmp_dir"' EXIT
    local archive="$tmp_dir/pdfium.tgz"
    local extract_dir="$tmp_dir/extract"
    mkdir -p "$extract_dir"

    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$PDFIUM_URL" -o "$archive" || return 1
    else
        wget -q "$PDFIUM_URL" -O "$archive" || return 1
    fi
    printf '%s  %s\n' "$PDFIUM_SHA256" "$archive" | sha256sum -c - || return 1
    tar -xzf "$archive" -C "$extract_dir" || return 1

    mkdir -p "$target_pdfium_dir"
    mkdir -p "$target_pdfium_dir/lib"
    cp -a "$extract_dir/lib/." "$target_pdfium_dir/lib/"
    cp -a "$extract_dir/LICENSE" "$extract_dir/licenses" "$target_pdfium_dir/"
)

