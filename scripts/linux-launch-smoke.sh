#!/usr/bin/env bash
set -euo pipefail

# Proves that a desktop binary stays running briefly without emitting a Rust
# panic or abort. This intentionally tests process liveness separately from
# diagnostics: the AccessKit regression previously left the window open while
# a background thread panicked.

if [[ $# -ne 1 ]]; then
  printf 'usage: %s /path/to/ottrin\n' "${0##*/}" >&2
  exit 2
fi

binary="$1"
if [[ ! -x "$binary" ]]; then
  printf 'Ottrin launch target is not executable: %s\n' "$binary" >&2
  exit 2
fi

scratch="$(mktemp -d)"
trap 'rm -rf -- "$scratch"' EXIT
log="$scratch/ottrin.log"

"$binary" >"$log" 2>&1 &
pid=$!
sleep 4

if ! kill -0 "$pid" 2>/dev/null; then
  wait "$pid" || true
  printf 'Ottrin exited before the Linux launch-smoke window.\n' >&2
  sed -n '1,160p' "$log" >&2
  exit 1
fi

kill -TERM "$pid" 2>/dev/null || true
wait "$pid" || true

if rg -n -i "thread '.*' panicked|panicked at|aborted \(core dumped\)|fatal runtime error" "$log" >&2; then
  printf 'Ottrin emitted a panic or abort during Linux launch smoke.\n' >&2
  exit 1
fi

printf 'Linux launch smoke passed: process remained alive without panic or abort output.\n'
