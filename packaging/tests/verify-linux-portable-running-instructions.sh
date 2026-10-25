#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERIFIER="$ROOT/packaging/verify-release-artifacts.sh"

require_literal() {
  local expected="$1"
  if ! grep -Fq -- "$expected" "$VERIFIER"; then
    printf 'release artifact verifier is missing Linux portable runtime guard: %s\n' \
      "$expected" >&2
    exit 1
  fi
}

# RUNNING.md is the only artifact-local explanation of the portable helper
# setup.  An artifact that omits or substitutes it must fail final verification
# rather than relying on the archive's checksum alone.
for expected in \
  "'/RUNNING\\.md$'" \
  'verify_linux_portable_running_instructions() {' \
  'Linux portable archive RUNNING.md' \
  'sudo ./libexec/ottrin/ottrin-priv-helper --install' \
  'Without the helper, normal user-owned file operations still work.'; do
  require_literal "$expected"
done

printf 'Verified Linux portable runtime-instructions contract.\n'
