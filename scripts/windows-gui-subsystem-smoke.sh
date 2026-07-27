#!/usr/bin/env bash
set -euo pipefail

# A Windows desktop launch must be a GUI PE image: Windows otherwise allocates
# a persistent console when the user starts Ottrin normally. This does not
# change CLI behavior when the executable is launched from an existing shell.
if [[ $# -ne 1 ]]; then
  printf 'usage: %s /path/to/ottrin.exe\n' "${0##*/}" >&2
  exit 2
fi

binary="$1"
if [[ ! -f "$binary" ]]; then
  printf 'Windows Ottrin artifact does not exist: %s\n' "$binary" >&2
  exit 2
fi

if command -v x86_64-w64-mingw32-objdump >/dev/null 2>&1; then
  objdump=x86_64-w64-mingw32-objdump
elif command -v objdump >/dev/null 2>&1; then
  objdump=objdump
else
  printf 'No objdump is available to inspect the Windows PE subsystem.\n' >&2
  exit 2
fi

pe_headers="$("$objdump" -p "$binary")"
if ! rg -q 'Subsystem[[:space:]]+00000002[[:space:]]+\(Windows GUI\)' <<<"$pe_headers"; then
  printf 'Windows Ottrin artifact must use the Windows GUI PE subsystem: %s\n' "$binary" >&2
  rg -i 'Subsystem' <<<"$pe_headers" >&2 || true
  exit 1
fi

printf 'Windows GUI subsystem smoke passed: ordinary launch allocates no console.\n'
