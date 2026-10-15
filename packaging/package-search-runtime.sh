#!/usr/bin/env bash
# One pinned live-content provider payload shared by the package builders.
package_search_runtime() (
    set -euo pipefail
    local platform="$1" bin_dir="$2" notice_dir="$3" manifest work
    manifest="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/search-runtime.json"
    local -a fields
    mapfile -d '' -t fields < <(python3 - "$manifest" "$platform" <<'PY'
import json,sys
m=json.load(open(sys.argv[1]));p=m['platforms'][sys.argv[2]]
for value in [m['version'],p['url'],p['sha256'],p['archive'],p['prefix'],p['binary'],p['binary_sha256'],*m['licenses']]:
    sys.stdout.buffer.write(value.encode()+b'\0')
PY
    )
    [[ "${#fields[@]}" == 10 ]] || { printf 'Invalid search-runtime manifest\n' >&2; exit 1; }
    local version="${fields[0]}" url="${fields[1]}" digest="${fields[2]}"
    local format="${fields[3]}" prefix="${fields[4]}" binary="${fields[5]}"
    work="$(mktemp -d)"
    trap 'rm -rf "$work"' EXIT
    mkdir -p "$work/extract" "$bin_dir" "$notice_dir"
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$url" -o "$work/archive"
    else
        wget -q "$url" -O "$work/archive"
    fi
    printf '%s  %s\n' "$digest" "$work/archive" | sha256sum -c -
    local -a members=("$prefix/$binary")
    local license
    for license in "${fields[@]:7}"; do members+=("$prefix/$license"); done
    case "$format" in
        tar.gz) tar -xzf "$work/archive" -C "$work/extract" -- "${members[@]}" ;;
        zip) unzip -q "$work/archive" "${members[@]}" -d "$work/extract" ;;
        *) printf 'Unsupported search-runtime archive: %s\n' "$format" >&2; exit 1 ;;
    esac
    printf '%s  %s\n' "${fields[6]}" "$work/extract/$prefix/$binary" | sha256sum -c -
    install -m 0755 "$work/extract/$prefix/$binary" "$bin_dir/$binary"
    for license in "${fields[@]:7}"; do
        install -m 0644 "$work/extract/$prefix/$license" "$notice_dir/$license"
    done
    python3 - "$manifest" "$notice_dir" <<'PY'
import hashlib,json,pathlib,shutil,sys
manifest=pathlib.Path(sys.argv[1]);data=json.loads(manifest.read_text())
for name,item in data['source_notices'].items():
    source=manifest.parent/item['file']
    assert hashlib.sha256(source.read_bytes()).hexdigest()==item['sha256'],name
    shutil.copyfile(source,pathlib.Path(sys.argv[2])/name)
PY
    printf 'ripgrep %s\nSource archive: %s\nArchive SHA-256: %s\nBinary SHA-256: %s\n' \
        "$version" "$url" "$digest" "${fields[6]}" > "$notice_dir/MANIFEST.txt"
)
