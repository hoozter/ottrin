#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WINDOWS_TARGET="${OTTRIN_TARGET:-x86_64-pc-windows-gnu}"
HOST_OS="$(uname -s)"

build_linux=true
build_appimage=true
build_windows=auto
build_macos=false
if [[ "$HOST_OS" == Darwin ]]; then
  build_linux=false
  build_appimage=false
  build_windows=false
  build_macos=true
fi

usage() {
  cat <<'EOF'
Build Ottrin release artifacts.

Usage:
  ./build-all.sh [options]

Options:
  --linux-only       Build Linux portable, AppImage, and native packages only.
  --macos-only       Build the macOS app archive only (requires macOS).
  --with-windows    Also build the Windows portable zip, NSIS setup executable,
                    and MSI. Fails if the Rust target, NSIS, or WiX is unavailable.
  --no-appimage     Skip the Linux AppImage.
  --no-linux        Skip Linux portable/AppImage/native artifacts.
  --help            Show this help.

Default:
  Builds Linux portable + Linux AppImage + Debian package, and builds the RPM
  when rpmbuild is available. Also builds Windows portable when the
  x86_64-pc-windows-gnu target is already installed.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --linux-only)
      build_windows=false
      build_macos=false
      ;;
    --macos-only)
      build_linux=false
      build_appimage=false
      build_windows=false
      build_macos=true
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
# Keep the local build entry point's deliverables together. Every packager
# receives the same destination, including standalone platform builds.
export OTTRIN_DIST_DIR="${OTTRIN_DIST_DIR:-$ROOT/dist/packages}"
mkdir -p "$OTTRIN_DIST_DIR"

artifacts=()
linux_helper_artifacts=()

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
  linux_helper_artifacts+=("portable=${artifacts[-1]}")
fi

if [[ "$build_appimage" == true ]]; then
  run_packager "Building Linux AppImage" "$ROOT/packaging/linux/package-appimage.sh"
  linux_helper_artifacts+=("appimage=${artifacts[-1]}")
fi

if [[ "$build_linux" == true ]]; then
  run_packager "Building native Debian package" \
    env OTTRIN_SKIP_BUILD=1 "$ROOT/packaging/linux/package-deb.sh"
  linux_helper_artifacts+=("deb=${artifacts[-1]}")
  if command -v rpmbuild >/dev/null 2>&1; then
    run_packager "Building native RPM package" \
      env OTTRIN_SKIP_BUILD=1 "$ROOT/packaging/linux/package-rpm.sh"
    linux_helper_artifacts+=("rpm=${artifacts[-1]}")
  else
    printf '\n==> Skipping native RPM package; rpmbuild is unavailable\n'
  fi
fi

if (( ${#linux_helper_artifacts[@]} )); then
  "$ROOT/packaging/tests/verify-linux-privileged-helper-package-path.sh" "${linux_helper_artifacts[@]}"
fi

if [[ "$build_macos" == true ]]; then
  if [[ "$HOST_OS" != Darwin ]]; then
    printf '\nmacOS app archives must be built on macOS.\n' >&2
    exit 1
  fi
  run_packager "Building macOS app archive" "$ROOT/packaging/macos/package-app.sh"
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
  if [[ "$build_windows" == true ]]; then
    run_packager "Building Windows NSIS installer" "$ROOT/packaging/windows/package-installer.sh"
    run_packager "Building Windows MSI" "$ROOT/packaging/windows/package-msi.sh"
  fi
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
