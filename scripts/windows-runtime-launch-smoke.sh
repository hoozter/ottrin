#!/usr/bin/env bash
set -euo pipefail

# Exercise a staged Windows GUI artifact, not only its PE headers.  Wine is a
# bounded compatibility probe: it proves that the process remains alive long
# enough to create its native window and that its captured diagnostics contain
# no Rust fatal startup output.  It is not a replacement for real-Windows QA.

if [[ $# -lt 1 || $# -gt 3 ]]; then
  printf 'usage: %s /path/to/ottrin.exe [--renderer opengl]\n' "${0##*/}" >&2
  exit 2
fi

binary="$1"
shift
if [[ ! -f "$binary" ]]; then
  printf 'Windows Ottrin artifact does not exist: %s\n' "$binary" >&2
  exit 2
fi
if ! command -v wine >/dev/null 2>&1; then
  printf 'Wine is unavailable; Windows runtime evidence is unavailable.\n' >&2
  exit 2
fi

renderer_args=()
if [[ $# -eq 2 ]]; then
  if [[ "$1" != '--renderer' || "$2" != 'opengl' ]]; then
    printf 'only the explicit OpenGL compatibility probe is supported\n' >&2
    exit 2
  fi
  renderer_args=(--renderer opengl)
elif [[ $# -ne 0 ]]; then
  printf 'only the explicit OpenGL compatibility probe is supported\n' >&2
  exit 2
fi

scratch="$(mktemp -d)"
trap 'rm -rf -- "$scratch"' EXIT
log="$scratch/ottrin-wine.log"

# /wait keeps Wine attached to the GUI child.  The timeout is intentionally
# longer than the liveness window so a healthy process is terminated only after
# it has been observed alive.
timeout --signal=TERM --kill-after=5s 15s \
  wine start /wait /unix "$binary" "${renderer_args[@]}" >"$log" 2>&1 &
pid=$!
sleep 4

if ! kill -0 "$pid" 2>/dev/null; then
  wait "$pid" || true
  printf 'Windows runtime probe: Ottrin exited before the liveness window.\n' >&2
  sed -n '1,200p' "$log" >&2
  exit 1
fi

kill -TERM "$pid" 2>/dev/null || true
wait "$pid" || true

if rg -n -i 'No wgpu backend feature|thread .+ panicked|panicked at|fatal runtime error|Unhandled exception' "$log" >&2; then
  printf 'Windows runtime probe found fatal startup diagnostics.\n' >&2
  exit 1
fi

printf 'Windows runtime launch smoke passed: process remained alive without fatal startup diagnostics.\n'
