#!/usr/bin/env bash
# Source-contract fixtures intentionally retain literal target-shell variables.
# shellcheck disable=SC2016
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
STAGER="$ROOT/packaging/linux/stage-native-package.sh"
DEB="$ROOT/packaging/linux/package-deb.sh"
RPM="$ROOT/packaging/linux/package-rpm.sh"
BUILD_ALL="$ROOT/build-all.sh"
PREFLIGHT="$ROOT/scripts/release-preflight.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
BIN="$TMP/bin"
STAGE="$TMP/stage"
mkdir -p "$BIN"

# Structural package fixtures use a local renderer payload. Real checksum refusal
# is exercised separately below, with the unmodified checksum utility.
mkdir -p "$TMP/pdfium/lib" "$TMP/pdfium/licenses" "$TMP/tools"
cp /bin/true "$TMP/pdfium/lib/libpdfium.so"
printf 'fixture license\n' > "$TMP/pdfium/LICENSE"
printf 'fixture license\n' > "$TMP/pdfium/licenses/pdfium.txt"
tar -czf "$TMP/pdfium.tgz" -C "$TMP/pdfium" .
export OTTRIN_TEST_PDFIUM_ARCHIVE="$TMP/pdfium.tgz"
cat > "$TMP/tools/curl" <<'MOCK'
#!/bin/sh
while [ "$#" -gt 0 ]; do
  if [ "$1" = '-o' ]; then cp "$OTTRIN_TEST_PDFIUM_ARCHIVE" "$2"; exit; fi
  shift
done
exit 1
MOCK
cat > "$TMP/tools/sha256sum" <<'MOCK'
#!/bin/sh
if [ "${1:-}" = '-c' ]; then cat >/dev/null; exit 0; fi
exec /usr/bin/sha256sum "$@"
MOCK
chmod +x "$TMP/tools/curl" "$TMP/tools/sha256sum"
ORIGINAL_PATH="$PATH"
export PATH="$TMP/tools:$PATH"

for executable in ottrin-app ottrin-indexd ottrin-priv-helper ottrin-media-playback ottrin-legacy-doc-render; do
  printf '#!/bin/sh\nexit 0\n' > "$BIN/$executable"
  chmod 0755 "$BIN/$executable"
done

OTTRIN_BUILD_DIR="$BIN" OTTRIN_SKIP_BUILD=1 "$STAGER" "$STAGE"

for executable in \
  usr/bin/ottrin \
  usr/bin/ottrin-indexd \
  usr/bin/ottrin-legacy-doc-render \
  usr/bin/ottrin-media-playback \
  usr/libexec/ottrin/ottrin-priv-helper; do
  test -x "$STAGE/$executable"
done
for payload in \
  usr/share/applications/org.ottrin.Ottrin.desktop \
  usr/share/icons/hicolor/scalable/apps/ottrin.svg \
  usr/share/icons/hicolor/256x256/apps/ottrin.png \
  usr/share/metainfo/org.ottrin.Ottrin.appdata.xml \
  usr/share/polkit-1/actions/org.ottrin.filesystem.policy \
  usr/share/ottrin/ottrin-index.service.in \
  usr/share/doc/ottrin/SOURCE_REVISION.txt \
  usr/libexec/ottrin/pdfium/lib/libpdfium.so \
  usr/libexec/ottrin/pdfium/LICENSE \
  usr/libexec/ottrin/pdfium/licenses/pdfium.txt; do
  test -f "$STAGE/$payload"
done

if find "$STAGE/usr/lib" -type f -print -quit 2>/dev/null | grep -q .; then
  printf 'native package staging bundled portable runtime libraries\n' >&2
  exit 1
fi

grep -Fq 'Exec=ottrin' "$STAGE/usr/share/applications/org.ottrin.Ottrin.desktop"
grep -Fq 'Git revision:' "$STAGE/usr/share/doc/ottrin/SOURCE_REVISION.txt"
for directory in usr usr/bin usr/share usr/share/doc/ottrin; do
  test "$(stat -c %a "$STAGE/$directory")" = 755
done
for packager in "$DEB" "$RPM"; do
  test -x "$packager"
  grep -Fq 'stage-native-package.sh' "$packager"
  grep -Fq 'sha256sum "$OUTPUT"' "$packager"
done
grep -Fq 'libglib2.0-0' "$DEB"
grep -Fq 'libfontconfig1' "$DEB"
for soname in \
  'libc.so.6()(64bit)' \
  'libgcc_s.so.1()(64bit)' \
  'libgio-2.0.so.0()(64bit)' \
  'libgobject-2.0.so.0()(64bit)' \
  'libglib-2.0.so.0()(64bit)' \
  'libgstreamer-1.0.so.0()(64bit)' \
  'libgstbase-1.0.so.0()(64bit)' \
  'libgstapp-1.0.so.0()(64bit)'; do
  grep -Fq "Requires: $soname" "$RPM"
done
grep -Fq 'package-deb.sh' "$BUILD_ALL"
grep -Fq 'package-rpm.sh' "$BUILD_ALL"
for covered in \
  'packaging/linux/stage-native-package.sh' \
  'packaging/linux/package-deb.sh' \
  'packaging/linux/package-rpm.sh' \
  'packaging/tests/verify-linux-native-package-contract.sh'; do
  grep -Fq "$covered" "$PREFLIGHT"
done
grep -Fq 'Linux native package contract' "$PREFLIGHT"

# Exercise the generated dependency, rather than only checking script text.
for executable in ottrin-app ottrin-indexd ottrin-priv-helper ottrin-media-playback ottrin-legacy-doc-render; do
  cp /bin/true "$BIN/$executable"
done
expected_libc="$(objdump -T /bin/true | sed -nE '/\*UND\*/s/.*\(GLIBC_([0-9]+(\.[0-9]+)+)\).*/\1/p' | sort -Vu | tail -n 1)"
test -n "$expected_libc"
OTTRIN_BUILD_DIR="$BIN" OTTRIN_SKIP_BUILD=1 OTTRIN_DIST_DIR="$TMP/dist" "$DEB"
depends="$(dpkg-deb -f "$TMP/dist/ottrin_0.1.0_amd64.deb" Depends)"
[[ "$depends" == "libc6 (>= $expected_libc),"* ]]
[[ ", $depends," == *", pkexec,"* ]]
[[ ", $depends," == *", polkitd,"* ]]
[[ ", $depends," == *", libxkbcommon-x11-0,"* ]]
for dependency in libxkbcommon-x11-0 libx11-6 libx11-xcb1 libxcursor1 libxi6; do
  [[ ", $depends," == *", $dependency,"* ]]
done
for soname in libxkbcommon-x11.so.0 libX11.so.6 libX11-xcb.so.1 libXcursor.so.1 libXi.so.6; do
  grep -Fxq "Requires: $soname()(64bit)" "$RPM"
done
grep -Fxq 'Requires: polkit' "$RPM"
grep -Fxq 'Requires: /usr/bin/pkexec' "$RPM"
# Reject a malformed helper even when the main binary is valid; keep the
# previously published package intact on packaging failure.
before="$(sha256sum "$TMP/dist/ottrin_0.1.0_amd64.deb")"
printf '#!/bin/sh\nexit 0\n' > "$BIN/ottrin-priv-helper"
if OTTRIN_BUILD_DIR="$BIN" OTTRIN_SKIP_BUILD=1 OTTRIN_DIST_DIR="$TMP/dist" "$DEB" > "$TMP/rejected.log" 2>&1; then
  printf 'DEB accepted a non-ELF helper\n' >&2
  exit 1
fi
[[ "$(sha256sum "$TMP/dist/ottrin_0.1.0_amd64.deb")" == "$before" ]]



# An untrusted local archive must fail the actual pinned checksum before staging.
if (
  export PATH="$ORIGINAL_PATH"
  source "$ROOT/packaging/linux/package-pdfium.sh"
  PDFIUM_URL="file://$TMP/pdfium.tgz"
  package_linux_pdfium "$TMP/rejected-pdfium"
); then
  printf 'PDFium package helper accepted a wrong archive checksum\n' >&2
  exit 1
fi
test ! -e "$TMP/rejected-pdfium"

printf 'Linux native package staging contract: PASS\n'
