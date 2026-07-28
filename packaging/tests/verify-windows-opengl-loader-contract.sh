#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION="$(grep -m1 '^version = ' "$ROOT/Cargo.toml" | sed -E 's/version = "([^"]+)"/\1/')"
STAGE="${1:-$ROOT/dist/Ottrin-$VERSION-windows-x64}"
EXPECTED_OPENGL32_SHA256="52119774eb912d07c3abfb6e3e894981f425b3c79d852f5363927e38fa3bde7e"

binary="$STAGE/bin/ottrin.exe"
loader="$STAGE/bin/opengl32.dll"
if [[ ! -f "$binary" ]]; then
  printf 'Windows OpenGL loader contract: missing application binary: %s\n' "$binary" >&2
  exit 1
fi
if [[ ! -f "$loader" ]]; then
  printf 'Windows OpenGL loader contract: missing app-local Mesa loader: %s\n' "$loader" >&2
  exit 1
fi
printf '%s  %s\n' "$EXPECTED_OPENGL32_SHA256" "$loader" | sha256sum -c - >/dev/null

if ! x86_64-w64-mingw32-objdump -p "$binary" | grep -Fi 'DLL Name: opengl32.dll' >/dev/null; then
  printf 'Windows OpenGL loader contract: ottrin.exe does not import opengl32.dll.\n' >&2
  exit 1
fi
if ! x86_64-w64-mingw32-objdump -p "$loader" | grep -Fi 'DLL Name: libgallium_wgl.dll' >/dev/null; then
  printf 'Windows OpenGL loader contract: Mesa opengl32.dll does not import libgallium_wgl.dll.\n' >&2
  exit 1
fi

printf 'Verified app-local Mesa OpenGL loader contract.\n'
