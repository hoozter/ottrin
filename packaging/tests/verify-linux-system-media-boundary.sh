#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
APPIMAGE_PACKAGER="$ROOT/packaging/linux/package-appimage.sh"
PORTABLE_PACKAGER="$ROOT/packaging/linux/package-portable.sh"
RUNTIME_HELPER="$ROOT/packaging/linux/bundle-runtime-libs.sh"
FINAL_VERIFIER="$ROOT/packaging/verify-release-artifacts.sh"
MEDIA_HELPER_SOURCE="$ROOT/crates/ottrin-preview/src/bin/ottrin-media-playback.rs"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf -- "$WORK_DIR"' EXIT

for packager in "$APPIMAGE_PACKAGER" "$PORTABLE_PACKAGER"; do
  grep -Fq 'cargo build --release -p ottrin-app -p ottrin-platform -p ottrin-search' "$packager" || {
    printf '%s does not build the GUI without the GStreamer feature.\n' "$packager" >&2
    exit 1
  }
  if grep -F 'cargo build --release -p ottrin-app' "$packager" | grep -Fq -- '--features system-gstreamer'; then
    printf '%s still links the GUI directly to GStreamer.\n' "$packager" >&2
    exit 1
  fi
  grep -Fq 'cargo build --release -p ottrin-preview --bin ottrin-media-playback --features system-gstreamer' "$packager" || {
    printf '%s does not build the optional system-GStreamer helper.\n' "$packager" >&2
    exit 1
  }
done

grep -Fq 'target/release/ottrin-media-playback' "$APPIMAGE_PACKAGER" || {
  echo 'AppImage does not install the media helper.' >&2
  exit 1
}
grep -Fq 'ottrin-media-playback" "$STAGE/libexec/ottrin/ottrin-media-playback' "$PORTABLE_PACKAGER" || {
  echo 'portable package does not install the media helper.' >&2
  exit 1
}
[[ -f "$MEDIA_HELPER_SOURCE" ]] || { echo 'media helper entry point is missing.' >&2; exit 1; }

grep -Fq 'verify_linux_media_runtime_boundary' "$FINAL_VERIFIER" || {
  echo 'final artifact verification does not enforce the media runtime boundary.' >&2
  exit 1
}

source "$RUNTIME_HELPER"
mkdir -p "$WORK_DIR/clean/lib" "$WORK_DIR/clean/libexec"
printf '#!/bin/sh\nexit 0\n' > "$WORK_DIR/clean/libexec/ottrin-media-playback"
chmod 0755 "$WORK_DIR/clean/libexec/ottrin-media-playback"
verify_linux_media_runtime_boundary 'clean fixture' "$WORK_DIR/clean" \
  "$WORK_DIR/clean/libexec/ottrin-media-playback" "$WORK_DIR/clean/lib"

mkdir -p "$WORK_DIR/contaminated/lib" "$WORK_DIR/contaminated/libexec"
cp "$WORK_DIR/clean/libexec/ottrin-media-playback" "$WORK_DIR/contaminated/libexec/"
printf 'bundled framework payload\n' > "$WORK_DIR/contaminated/lib/libgstreamer-1.0.so.0"
if verify_linux_media_runtime_boundary 'contaminated fixture' "$WORK_DIR/contaminated" \
  "$WORK_DIR/contaminated/libexec/ottrin-media-playback" "$WORK_DIR/contaminated/lib" \
  >"$WORK_DIR/stdout" 2>"$WORK_DIR/stderr"; then
  echo 'media runtime verifier accepted bundled GStreamer.' >&2
  exit 1
fi
grep -Fq 'contains a bundled media framework or codec payload' "$WORK_DIR/stderr" || {
  echo 'media runtime verifier rejected bundled GStreamer for the wrong reason.' >&2
  cat "$WORK_DIR/stderr" >&2
  exit 1
}

printf 'Verified Linux system-media helper and no-bundling contract.\n'
