#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERIFIER="$ROOT/packaging/verify-release-artifacts.sh"

require_literal() {
  local literal="$1"
  if ! grep -Fq -- "$literal" "$VERIFIER"; then
    printf 'missing Linux privileged-helper policy integrity guard: %s\n' "$literal" >&2
    exit 1
  fi
}

# A policy path alone is not safe evidence: a substituted package could retain
# the expected filename while changing authorization semantics. Both Linux
# artifact formats must retain the reviewed policy bytes.
for literal in \
  'verify_linux_privileged_helper_policy_bytes() {' \
  'contains stale privileged-helper policy.' \
  'packaging/linux/org.ottrin.filesystem.policy'; do
  require_literal "$literal"
done

printf 'Verified Linux privileged-helper policy integrity contract.\n'
