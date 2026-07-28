#!/usr/bin/env bash
set -euo pipefail

# Exercise a staged Windows GUI artifact, not only its PE headers.  Wine is a
# bounded compatibility probe: it checks wrapper liveness, Ottrin's durable
# startup log, and the loaded OpenGL module. It is not a replacement for
# real-Windows QA.

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
opengl_probe=false
if [[ $# -eq 2 ]]; then
  if [[ "$1" != '--renderer' || "$2" != 'opengl' ]]; then
    printf 'only the explicit OpenGL compatibility probe is supported\n' >&2
    exit 2
  fi
  renderer_args=(--renderer opengl)
  opengl_probe=true
elif [[ $# -ne 0 ]]; then
  printf 'only the explicit OpenGL compatibility probe is supported\n' >&2
  exit 2
fi

if [[ "$opengl_probe" == true ]]; then
  loader="$(dirname "$binary")/opengl32.dll"
  expected_loader_sha256="52119774eb912d07c3abfb6e3e894981f425b3c79d852f5363927e38fa3bde7e"
  if [[ ! -f "$loader" ]]; then
    printf 'Windows OpenGL runtime probe: app-local Mesa opengl32.dll is missing.\n' >&2
    exit 1
  fi
  printf '%s  %s\n' "$expected_loader_sha256" "$loader" | sha256sum -c - >/dev/null
fi

scratch="$(mktemp -d)"
trap 'rm -rf -- "$scratch"' EXIT
log="$scratch/ottrin-wine.log"

wine_prefix="${WINEPREFIX:-$HOME/.wine}"
startup_logs=()
startup_log_sizes=()
for startup_log in "$wine_prefix"/drive_c/users/*/AppData/Local/[Oo]ttrin/startup.log; do
  [[ -f "$startup_log" ]] || continue
  startup_logs+=("$startup_log")
  startup_log_sizes+=("$(wc -c < "$startup_log")")
done

# /wait keeps Wine attached to the GUI child.  The timeout is intentionally
# longer than the liveness window so a healthy process is terminated only after
# it has been observed alive.
wine_debug=-all
if [[ "$opengl_probe" == true ]]; then
  wine_debug=+loaddll
fi
timeout --signal=TERM --kill-after=5s 15s \
  env WINEDEBUG="$wine_debug" \
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

for startup_log in "$wine_prefix"/drive_c/users/*/AppData/Local/[Oo]ttrin/startup.log; do
  [[ -f "$startup_log" ]] || continue
  previous_size=0
  for index in "${!startup_logs[@]}"; do
    if [[ "${startup_logs[$index]}" == "$startup_log" ]]; then
      previous_size="${startup_log_sizes[$index]}"
      break
    fi
  done
  appended="$scratch/startup-$RANDOM.log"
  dd if="$startup_log" of="$appended" bs=1 skip="$previous_size" status=none
  if grep -Fq 'Ottrin startup failure' "$appended"; then
    printf 'Windows runtime probe found a durable Ottrin startup failure.\n' >&2
    sed -n '1,80p' "$appended" >&2
    exit 1
  fi
done

if grep -Eni 'No wgpu backend feature|thread .+ panicked|panicked at|fatal runtime error|Unhandled exception|egui_glow: OpenGL|requires opengl' "$log" >&2; then
  printf 'Windows runtime probe found fatal startup diagnostics.\n' >&2
  exit 1
fi

if [[ "$opengl_probe" == true ]]; then
  if grep -Eqi 'windows\\\\system32\\\\opengl32\.dll' "$log"; then
    printf 'Windows OpenGL runtime probe loaded the system opengl32.dll instead of the bundled Mesa loader.\n' >&2
    exit 1
  fi
  if ! grep -Eqi 'Loaded .+opengl32\.dll' "$log"; then
    printf 'Windows OpenGL runtime probe could not prove which opengl32.dll was loaded.\n' >&2
    exit 1
  fi
fi

printf 'Windows runtime launch smoke passed: process remained alive without fatal startup diagnostics.\n'
