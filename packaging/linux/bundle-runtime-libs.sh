#!/usr/bin/env bash
# Shared, fail-closed Linux runtime closure bundler for release artifacts.
set -euo pipefail

linux_runtime_needed() {
  readelf -d "$1" | awk '/Shared library: \[/{ sub(/^.*\[/, ""); sub(/\].*$/, ""); print }'
}

# These SONAMEs are supplied by the supported host ABI: the system dynamic
# loader, glibc (including its split components), and the base C++ runtime.
# Keep this single classifier shared by the bundler and final-artifact verifier.
linux_runtime_is_host_base_abi() {
  case "$1" in
    ld-linux-x86-64.so.2|libc.so.6|libdl.so.2|libm.so.6|libpthread.so.0|librt.so.1|libgcc_s.so.1|libstdc++.so.6|libresolv.so.2|libutil.so.1|libanl.so.1)
      return 0
      ;;
    *) return 1 ;;
  esac
}

# The playback helper intentionally resolves the user's system media stack at
# runtime. The GUI and its portable closure must remain independent of that
# stack so a missing GStreamer installation cannot prevent Ottrin from
# launching. Reject framework, plugin, and codec payloads anywhere in the
# artifact rather than relying on an incomplete library exclusion list.
verify_linux_media_runtime_boundary() {
  local artifact_label="$1"
  local artifact_root="$2"
  local media_helper="$3"
  local lib_dir="$4"
  local canonical_root canonical_helper canonical_lib_dir

  [[ -d "$artifact_root" && ! -L "$artifact_root" && -x "$media_helper" && -f "$media_helper" \
    && ! -L "$media_helper" && -d "$lib_dir" && ! -L "$lib_dir" ]] || {
    printf '%s is missing a trusted media helper or runtime root.\n' "$artifact_label" >&2
    return 1
  }
  canonical_root="$(realpath -e -- "$artifact_root")" || return 1
  canonical_helper="$(realpath -e -- "$media_helper")" || return 1
  canonical_lib_dir="$(realpath -e -- "$lib_dir")" || return 1
  [[ "$canonical_root" != / && "$canonical_helper" == "$canonical_root/"* \
    && "$canonical_lib_dir" == "$canonical_root/"* ]] || {
    printf '%s has an unsafe media runtime root.\n' "$artifact_label" >&2
    return 1
  }

  local payload basename_lower
  while IFS= read -r -d '' payload; do
    basename_lower="$(basename -- "$payload" | tr '[:upper:]' '[:lower:]')"
    case "$basename_lower" in
      *gstreamer*|libgst*|*gst-plugin*|*avcodec*|*avformat*|*avutil*|*swresample*|*swscale*|*ffmpeg*)
        printf '%s contains a bundled media framework or codec payload: %s\n' \
          "$artifact_label" "${payload#"$canonical_root"/}" >&2
        return 1
        ;;
    esac
  done < <(find "$canonical_root" -type f -print0)
}

# Verify the runtime closure from an extracted artifact. This deliberately
# lives beside the builder so both behavioral tests and the final release
# verifier enforce the same format and safety boundary. The manifest binds
# shipped bytes and notices to an unsigned build record for integrity and
# diagnosis; it is not a final-artifact authenticity claim.
verify_linux_runtime_closure() {
  local artifact_label="$1"
  local artifact_root="$2"
  local executable="$3"
  local lib_dir="$4"
  local manifest="$5"
  local notice_root="$6"
  local canonical_artifact_root canonical_executable canonical_lib_dir canonical_manifest canonical_notice_root
  local -A manifest_sonames=()
  local -A package_notices=()
  local -A required_sonames=()
  local -A visited_elfs=()
  local -a elf_queue=()

  command -v readelf >/dev/null 2>&1 || { echo 'readelf is required to verify a Linux runtime closure.' >&2; return 1; }
  command -v dpkg >/dev/null 2>&1 || { echo 'dpkg is required to validate Linux runtime provenance.' >&2; return 1; }
  [[ -d "$artifact_root" && ! -L "$artifact_root" && -x "$executable" && -f "$executable" && ! -L "$executable" && -d "$lib_dir" && ! -L "$lib_dir" && -f "$manifest" && ! -L "$manifest" && -d "$notice_root" && ! -L "$notice_root" ]] || {
    printf '%s is missing a trusted artifact root, regular executable, library directory, manifest, or notice root.\n' "$artifact_label" >&2
    return 1
  }
  canonical_artifact_root="$(realpath -e -- "$artifact_root")" || return 1
  canonical_executable="$(realpath -e -- "$executable")" || return 1
  canonical_lib_dir="$(realpath -e -- "$lib_dir")" || return 1
  canonical_manifest="$(realpath -e -- "$manifest")" || return 1
  canonical_notice_root="$(realpath -e -- "$notice_root")" || return 1
  [[ "$canonical_artifact_root" != / && "$canonical_lib_dir" != / && "$canonical_notice_root" != / \
    && "$canonical_executable" == "$canonical_artifact_root/"* \
    && "$canonical_lib_dir" == "$canonical_artifact_root/"* \
    && "$canonical_manifest" == "$canonical_artifact_root/"* \
    && "$canonical_notice_root" == "$canonical_artifact_root/"* ]] || {
    printf '%s has an unsafe runtime root.\n' "$artifact_label" >&2
    return 1
  }

  linux_runtime_reject_search_path() {
    local path="$1"
    if readelf -d "$path" | grep -Eq '\((RPATH|RUNPATH)\)'; then
      printf '%s contains an unsupported ELF RPATH/RUNPATH: %s\n' "$artifact_label" "$path" >&2
      return 1
    fi
  }
  linux_runtime_safe_notice_path() {
    local path="$1"
    [[ "$path" != /* && "$path" != *'//' && "$path" != '.' && "$path" != '..' && "$path" != ../* && "$path" != */../* && "$path" != */.. ]] || return 1
  }
  linux_runtime_valid_package() {
    [[ "$1" =~ ^[a-z0-9][a-z0-9+.-]*(:[a-z0-9][a-z0-9-]*)?$ ]]
  }
  linux_runtime_reject_search_path "$canonical_executable" || return 1
  if ! readelf -d "$canonical_executable" | grep -Fq 'Shared library: [libgio-2.0.so.0]'; then
    printf '%s main executable does not declare libgio-2.0.so.0.\n' "$artifact_label" >&2
    return 1
  fi

  local soname library_sha256 package package_version notice_relative notice_sha256 extra
  local line_number=0 header_seen=0
  while IFS= read -r line || [[ -n "$line" ]]; do
    line_number=$((line_number + 1))
    if [[ "$line_number" -eq 1 && "$line" != '# Ottrin Linux runtime closure unsigned build record (integrity metadata; not artifact authenticity).' ]]; then
      if [[ "$line" == *$'\t'* ]]; then
        printf '%s is missing the unsigned build-record manifest header.\n' "$artifact_label" >&2
      else
        printf '%s has a malformed runtime manifest header.\n' "$artifact_label" >&2
      fi
      return 1
    fi
    if [[ "$line" == '# Ottrin Linux runtime closure unsigned build record (integrity metadata; not artifact authenticity).' ]]; then
      [[ "$line_number" -eq 1 && "$header_seen" -eq 0 ]] || {
        printf '%s has a malformed runtime manifest header.\n' "$artifact_label" >&2
        return 1
      }
      header_seen=1
      continue
    fi
    # A tab is whitespace to Bash's `read`, which would collapse empty fields
    # and misclassify a missing version as a later-column failure. Parse via a
    # non-whitespace temporary delimiter so every manifest column stays bound.
    local manifest_separator=$'\x1f'
    IFS="$manifest_separator" read -r soname library_sha256 package package_version notice_relative notice_sha256 extra \
      <<<"${line//$'\t'/$manifest_separator}"
    if [[ -n "$extra" || -z "$soname" || -z "$library_sha256" || -z "$notice_relative" || -z "$notice_sha256" ]] \
      || ! [[ "$soname" =~ ^[A-Za-z0-9._+-]+$ && "$library_sha256" =~ ^[0-9a-f]{64}$ && "$notice_sha256" =~ ^[0-9a-f]{64}$ ]]; then
      printf '%s has a malformed runtime manifest row %d.\n' "$artifact_label" "$line_number" >&2
      return 1
    fi
    if ! linux_runtime_valid_package "$package"; then
      printf '%s has an invalid runtime manifest package at row %d: %s\n' "$artifact_label" "$line_number" "${package:-empty}" >&2
      return 1
    fi
    if [[ -z "$package_version" ]] || ! dpkg --validate-version "$package_version"; then
      printf '%s has an invalid runtime manifest package version at row %d.\n' "$artifact_label" "$line_number" >&2
      return 1
    fi
    if ! linux_runtime_safe_notice_path "$notice_relative"; then
      printf '%s has an unsafe runtime notice path at row %d: %s\n' "$artifact_label" "$line_number" "$notice_relative" >&2
      return 1
    fi
    [[ -z "${manifest_sonames[$soname]+x}" ]] || {
      printf '%s has duplicate runtime manifest SONAME: %s\n' "$artifact_label" "$soname" >&2
      return 1
    }
    manifest_sonames[$soname]=1

    local library="$canonical_lib_dir/$soname"
    local canonical_library
    canonical_library="$(realpath -e -- "$library" 2>/dev/null)" || canonical_library=''
    [[ "$canonical_library" == "$canonical_lib_dir/$soname" && -f "$library" && ! -L "$library" ]] || {
      printf '%s runtime library is missing or symlinked: %s\n' "$artifact_label" "$soname" >&2
      return 1
    }
    [[ "$(sha256sum "$canonical_library" | awk '{print $1}')" == "$library_sha256" ]] || {
      printf '%s runtime library digest mismatch: %s\n' "$artifact_label" "$soname" >&2
      return 1
    }
    local embedded_soname
    embedded_soname="$(readelf -d "$canonical_library" | awk '/SONAME.*\[/{ sub(/^.*\[/, ""); sub(/\].*$/, ""); print; exit }')"
    [[ "$embedded_soname" == "$soname" ]] || {
      printf '%s runtime library SONAME mismatch: %s\n' "$artifact_label" "$soname" >&2
      return 1
    }
    linux_runtime_reject_search_path "$canonical_library" || return 1

    local notice="$canonical_notice_root/$notice_relative"
    local canonical_notice
    canonical_notice="$(realpath -e -- "$notice" 2>/dev/null)" || {
      printf '%s is missing runtime notice: %s\n' "$artifact_label" "$notice_relative" >&2
      return 1
    }
    [[ "$canonical_notice" == "$canonical_notice_root/"* && -f "$notice" && ! -L "$notice" ]] || {
      printf '%s runtime notice is unsafe: %s\n' "$artifact_label" "$notice_relative" >&2
      return 1
    }
    [[ "$(sha256sum "$notice" | awk '{print $1}')" == "$notice_sha256" ]] || {
      printf '%s runtime notice digest mismatch: %s\n' "$artifact_label" "$notice_relative" >&2
      return 1
    }
    if [[ -n "${package_notices[$package]+x}" && "${package_notices[$package]}" != "$package_version:$notice_relative:$notice_sha256" ]]; then
      printf '%s has inconsistent runtime notice provenance for package: %s\n' "$artifact_label" "$package" >&2
      return 1
    fi
    package_notices[$package]="$package_version:$notice_relative:$notice_sha256"
  done < "$manifest"

  [[ "$header_seen" -eq 1 ]] || {
    printf '%s is missing the unsigned build-record manifest header.\n' "$artifact_label" >&2
    return 1
  }

  [[ -n "${manifest_sonames[libgio-2.0.so.0]+x}" ]] || {
    printf '%s runtime manifest is missing required libgio-2.0.so.0.\n' "$artifact_label" >&2
    return 1
  }

  elf_queue=("$canonical_executable")
  while (( ${#elf_queue[@]} )); do
    local current_elf="${elf_queue[0]}"
    elf_queue=("${elf_queue[@]:1}")
    [[ -z "${visited_elfs[$current_elf]+x}" ]] || continue
    visited_elfs[$current_elf]=1

    local needed_soname
    while IFS= read -r needed_soname; do
      [[ -n "$needed_soname" ]] || continue
      linux_runtime_is_host_base_abi "$needed_soname" && continue
      required_sonames[$needed_soname]=1
      [[ -n "${manifest_sonames[$needed_soname]+x}" ]] || {
        printf '%s is missing a runtime manifest row for required %s.\n' "$artifact_label" "$needed_soname" >&2
        return 1
      }
      elf_queue+=("$canonical_lib_dir/$needed_soname")
    done < <(linux_runtime_needed "$current_elf")
  done

  local trace resolved canonical_resolved
  trace="$(env -i LD_TRACE_LOADED_OBJECTS=1 LD_LIBRARY_PATH="$canonical_lib_dir" "$canonical_executable" 2>&1)" || {
    printf '%s hermetic loader trace failed.\n' "$artifact_label" >&2
    return 1
  }
  for soname in "${!required_sonames[@]}"; do
    resolved="$(awk -v soname="$soname" '$1 == soname && $2 == "=>" { print $3; exit }' <<<"$trace")"
    canonical_resolved="$(realpath -e -- "$resolved" 2>/dev/null)" || canonical_resolved=''
    [[ "$canonical_resolved" == "$canonical_lib_dir/"* ]] || {
      printf '%s resolves bundled %s outside its artifact: %s\n' "$artifact_label" "$soname" "${resolved:-missing}" >&2
      return 1
    }
  done
}

bundle_linux_runtime() {
  local executable="$1"
  local artifact_root="$2"
  local lib_dir="$artifact_root/lib"
  local notices_dir="${3:-$artifact_root/share/doc/ottrin/third-party-notices/linux-runtime}"
  local manifest="$notices_dir/BUNDLED_RUNTIME_MANIFEST.txt"
  local -a queue=()
  local -A seen_sonames=()
  local -A copied_sonames=()
  local -A copied_packages=()
  local executable_class executable_machine

  command -v readelf >/dev/null 2>&1 || { echo 'readelf is required to bundle Linux runtime libraries.' >&2; return 1; }
  command -v ldconfig >/dev/null 2>&1 || { echo 'ldconfig is required to bundle Linux runtime libraries.' >&2; return 1; }
  command -v dpkg-query >/dev/null 2>&1 || { echo 'dpkg-query is required to record Linux runtime provenance.' >&2; return 1; }
  [[ "$executable" = /* && -f "$executable" && ! -L "$executable" ]] || {
    printf 'Trusted executable must be an absolute regular non-symlink path: %s\n' "$executable" >&2
    return 1
  }
  executable_class="$(readelf -h "$executable" | awk -F: '/Class:/{ gsub(/^[[:space:]]+/, "", $2); print $2; exit }')"
  executable_machine="$(readelf -h "$executable" | awk -F: '/Machine:/{ gsub(/^[[:space:]]+/, "", $2); print $2; exit }')"
  [[ -n "$executable_class" && -n "$executable_machine" ]] || {
    printf 'Unable to identify ELF architecture for trusted executable: %s\n' "$executable" >&2
    return 1
  }

  mkdir -p "$lib_dir" "$notices_dir"
  printf '%s\n' '# Ottrin Linux runtime closure unsigned build record (integrity metadata; not artifact authenticity).' > "$manifest"

  linux_runtime_resolve() {
    local soname="$1"
    local -a matches=()
    mapfile -t matches < <(
      ldconfig -p | awk -v soname="$soname" '$1 == soname && $NF ~ /^\// { print $NF }' \
        | while IFS= read -r candidate; do
            candidate="$(realpath -e -- "$candidate")" || continue
            [[ -f "$candidate" && ! -L "$candidate" ]] || continue
            candidate_class="$(readelf -h "$candidate" | awk -F: '/Class:/{ gsub(/^[[:space:]]+/, "", $2); print $2; exit }')"
            candidate_machine="$(readelf -h "$candidate" | awk -F: '/Machine:/{ gsub(/^[[:space:]]+/, "", $2); print $2; exit }')"
            [[ "$candidate_class" == "$executable_class" && "$candidate_machine" == "$executable_machine" ]] || continue
            printf '%s\n' "$candidate"
          done \
        | sort -u
    )
    if (( ${#matches[@]} != 1 )); then
      printf 'Unable to resolve exactly one host library for %s (found %d).\n' "$soname" "${#matches[@]}" >&2
      return 1
    fi
    realpath -e -- "${matches[0]}"
  }

  queue=("$executable")
  while (( ${#queue[@]} )); do
    local current="${queue[0]}"
    queue=("${queue[@]:1}")
    local soname
    while IFS= read -r soname; do
      [[ -n "$soname" ]] || continue
      if linux_runtime_is_host_base_abi "$soname"; then
        continue
      fi
      [[ -z "${seen_sonames[$soname]+x}" ]] || continue
      seen_sonames[$soname]=1

      local source
      source="$(linux_runtime_resolve "$soname")" || return 1
      [[ "$source" = /* && -f "$source" && ! -L "$source" ]] || {
        printf 'Resolved library must be an absolute regular non-symlink path: %s\n' "$source" >&2
        return 1
      }

      local declared_soname
      declared_soname="$(readelf -d "$source" | awk '/SONAME.*\[/{ sub(/^.*\[/, ""); sub(/\].*$/, ""); print; exit }')"
      [[ "$declared_soname" == "$soname" ]] || {
        printf 'Resolved library SONAME mismatch: requested %s, found %s in %s\n' "$soname" "${declared_soname:-none}" "$source" >&2
        return 1
      }

      local destination="$lib_dir/$soname"
      if [[ -e "$destination" ]]; then
        cmp -s "$source" "$destination" || {
          printf 'Library copy collision has different content: %s\n' "$destination" >&2
          return 1
        }
      else
        install -m 0644 "$source" "$destination"
      fi
      copied_sonames[$soname]=1
      queue+=("$source")

      local package
      package="$(dpkg-query -S "$source" 2>/dev/null | sed -n '1{s/: [^:]*$//;p}')"
      [[ -n "$package" ]] || {
        printf 'Unable to identify Debian package provenance for %s\n' "$source" >&2
        return 1
      }
      local version
      version="$(dpkg-query -W -f='${Version}' "$package" 2>/dev/null)" || {
        printf 'Unable to identify Debian package version for %s\n' "$package" >&2
        return 1
      }
      if [[ -z "${copied_packages[$package]+x}" ]]; then
        copied_packages[$package]=1
        local package_base="${package%%:*}"
        local copyright="/usr/share/doc/$package_base/copyright"
        [[ -f "$copyright" && ! -L "$copyright" ]] || {
          printf 'Missing package copyright notice for bundled runtime %s: %s\n' "$package" "$copyright" >&2
          return 1
        }
        install -m 0644 "$copyright" "$notices_dir/$package_base.copyright"
      fi
      local notice_relative="$package_base.copyright"
      printf '%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$soname" "$(sha256sum "$destination" | awk '{print $1}')" "$package" "$version" \
        "$notice_relative" "$(sha256sum "$notices_dir/$notice_relative" | awk '{print $1}')" >> "$manifest"
    done < <(linux_runtime_needed "$current")
  done

  [[ -n "${copied_sonames[libgio-2.0.so.0]+x}" ]] || {
    echo 'Bundled runtime closure is missing required libgio-2.0.so.0.' >&2
    return 1
  }
}
