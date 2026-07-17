#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WINDOWS_TARGET="${OTTRIN_TARGET:-x86_64-pc-windows-gnu}"

build_linux=true
build_appimage=true
build_windows=auto

usage() {
  cat <<'EOF'
Build Ottrin release artifacts.

Usage:
  ./build-all.sh [options]

Options:
  --linux-only       Build Linux portable and AppImage only.
  --with-windows    Also build the Windows portable zip. Fails if the target is unavailable.
  --no-appimage     Skip the Linux AppImage.
  --no-linux        Skip Linux portable/AppImage artifacts.
  --help            Show this help.

Default:
  Builds Linux portable + Linux AppImage. Also builds Windows portable when
  the x86_64-pc-windows-gnu target is already installed.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --linux-only)
      build_windows=false
      ;;
    --with-windows)
      build_windows=true
      ;;
    --no-appimage)
      build_appimage=false
      ;;
    --no-linux)
      build_linux=false
      build_appimage=false
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      printf 'Unknown option: %s\n\n' "$1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

cd "$ROOT"
mkdir -p "$ROOT/dist"

artifacts=()

run_packager() {
  local label="$1"
  shift
  local output_file
  output_file="$(mktemp)"
  printf '\n==> %s\n' "$label"
  "$@" | tee "$output_file"
  local artifact
  artifact="$(tail -n1 "$output_file")"
  rm -f "$output_file"
  artifacts+=("$artifact")
}

if [[ "$build_linux" == true ]]; then
  run_packager "Building Linux portable tarball" "$ROOT/packaging/linux/package-portable.sh"
fi

if [[ "$build_appimage" == true ]]; then
  run_packager "Building Linux AppImage" "$ROOT/packaging/linux/package-appimage.sh"
fi

windows_target_installed=false
if rustup target list --installed 2>/dev/null | grep -qx "$WINDOWS_TARGET"; then
  windows_target_installed=true
fi

if [[ "$build_windows" == true || ( "$build_windows" == auto && "$windows_target_installed" == true ) ]]; then
  if [[ "$windows_target_installed" != true ]]; then
    printf '\nWindows target is not installed: %s\n' "$WINDOWS_TARGET" >&2
    printf 'Install it with: rustup target add %s\n' "$WINDOWS_TARGET" >&2
    exit 1
  fi
  run_packager "Building Windows portable zip" "$ROOT/packaging/windows/package-portable.sh"
elif [[ "$build_windows" == auto ]]; then
  printf '\n==> Skipping Windows portable zip; target not installed (%s)\n' "$WINDOWS_TARGET"
  printf '    Run ./build-all.sh --with-windows after: rustup target add %s\n' "$WINDOWS_TARGET"
fi

printf '\nBuilt artifacts:\n'
for artifact in "${artifacts[@]}"; do
  printf '  %s\n' "$artifact"
  if [[ -f "$artifact.sha256" ]]; then
    printf '  %s\n' "$artifact.sha256"
  fi
done
