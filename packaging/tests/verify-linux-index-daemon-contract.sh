#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
UNIT="$ROOT/packaging/linux/ottrin-index.service.in"
PORTABLE="$ROOT/packaging/linux/package-portable.sh"
APPIMAGE="$ROOT/packaging/linux/package-appimage.sh"

for expected in \
  'Type=simple' \
  'ExecStart=@OTTRIN_EXECUTABLE@ --index-daemon' \
  'Restart=on-failure' \
  'WantedBy=default.target'; do
  grep -Fq "$expected" "$UNIT"
done

if grep -Eq 'Type=oneshot|index update|\[Timer\]' "$UNIT"; then
  printf 'persistent daemon unit regressed to periodic refresh semantics\n' >&2
  exit 1
fi

grep -Fq 'ottrin-index.service.in' "$PORTABLE"
grep -Fq 'ottrin-index.service.in' "$APPIMAGE"
grep -Fq '$BUILD_DIR/ottrin-indexd" "$STAGE/libexec/ottrin/ottrin-indexd' "$PORTABLE"
grep -Fq '$ROOT/target/release/ottrin-indexd" "$APPDIR/usr/bin/ottrin-indexd' "$APPIMAGE"
grep -Fq 'SEARCH_PERFORMANCE.md' "$PORTABLE"
grep -Fq 'SEARCH_PERFORMANCE.md' "$APPIMAGE"
printf 'linux index daemon packaging contract: PASS\n'
