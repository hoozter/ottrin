#!/usr/bin/env bash
# Verify the same pinned content provider that ships in Ottrin before testing.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
: "${RUNNER_TEMP:?A disposable runner directory is required}"
: "${GITHUB_RUN_ID:?A job identity is required}"
: "${GITHUB_JOB:?A job identity is required}"
case "${RUNNER_OS:?}" in
    Linux) platform=linux-x86_64; binary=rg; runner_temp="$RUNNER_TEMP" ;;
    Windows) platform=windows-x86_64; binary=rg.exe; runner_temp="$(cygpath -u "$RUNNER_TEMP")" ;;
    *) printf 'Unsupported search-provider runner: %s\n' "$RUNNER_OS" >&2; exit 1 ;;
esac
[[ "${RUNNER_ARCH:?}" == X64 ]] || { printf 'Expected an X64 runner\n' >&2; exit 1; }
stage="$runner_temp/ottrin-search-runtime"
owner="$GITHUB_RUN_ID:$GITHUB_JOB:$RUNNER_OS"

cleanup() {
    if [[ ! -e "$stage" && ! -L "$stage" ]]; then return; fi
    [[ -d "$stage" && ! -L "$stage" && -f "$stage/OWNER" && ! -L "$stage/OWNER" && "$(cat "$stage/OWNER")" == "$owner" ]] || {
        printf 'Refusing unrelated search-provider staging: %s\n' "$stage" >&2; return 1;
    }
    rm -rf -- "$stage"
}

case "${1:-prepare}" in
    cleanup) cleanup; exit ;;
    prepare) ;;
    *) printf 'Expected prepare or cleanup\n' >&2; exit 1 ;;
esac
: "${GITHUB_PATH:?The runner path export is required}"
[[ ! -e "$stage" && ! -L "$stage" ]] || { printf 'Search-provider staging already exists\n' >&2; exit 1; }
ready=false
trap 'if [[ "$ready" != true ]]; then cleanup; fi' EXIT
mkdir "$stage"
printf '%s\n' "$owner" > "$stage/OWNER"
source "$ROOT/packaging/package-search-runtime.sh"
package_search_runtime "$platform" "$stage/bin" "$stage/notices"
"$stage/bin/$binary" --version
printf 'ottrin-provider-check\n' > "$stage/probe.txt"
"$stage/bin/$binary" --no-config --fixed-strings --quiet ottrin-provider-check "$stage/probe.txt"
rm "$stage/probe.txt"
if [[ "$RUNNER_OS" == Windows ]]; then
    cygpath -w "$stage/bin" >> "$GITHUB_PATH"
else
    printf '%s\n' "$stage/bin" >> "$GITHUB_PATH"
fi
ready=true
