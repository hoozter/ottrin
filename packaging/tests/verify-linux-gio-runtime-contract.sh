#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
HELPER="$ROOT/packaging/linux/bundle-runtime-libs.sh"
FINAL_VERIFIER="$ROOT/packaging/verify-release-artifacts.sh"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf -- "$WORK_DIR"' EXIT

grep -Fq 'verify_linux_runtime_closure' "$HELPER" || { echo 'missing shared strict runtime verifier.' >&2; exit 1; }
# The fixture checks source text, not this process's ROOT value.
# shellcheck disable=SC2016
grep -Fq 'source "$ROOT/packaging/linux/bundle-runtime-libs.sh"' "$FINAL_VERIFIER" || { echo 'final verifier does not source the shared runtime verifier.' >&2; exit 1; }
grep -Fq "verify_linux_runtime_closure \\" "$FINAL_VERIFIER" || { echo 'final verifier does not invoke the shared runtime verifier.' >&2; exit 1; }

command -v gcc >/dev/null 2>&1 || { echo 'gcc is required for the Linux runtime closure behavioral test.' >&2; exit 1; }
command -v pkg-config >/dev/null 2>&1 || { echo 'pkg-config is required for the Linux runtime closure behavioral test.' >&2; exit 1; }
pkg-config --exists gio-2.0 || { echo 'gio-2.0 development metadata is required for the Linux runtime closure behavioral test.' >&2; exit 1; }

# shellcheck source=packaging/linux/bundle-runtime-libs.sh
source "$HELPER"

cat > "$WORK_DIR/gio-smoke.c" <<'EOF'
#include <gio/gio.h>
int main(void) {
  gboolean uncertain = FALSE;
  gchar *content_type = g_content_type_guess("ottrin-runtime-smoke.txt", NULL, 0, &uncertain);
  if (content_type == NULL) return 1;
  g_free(content_type);
  return 0;
}
EOF
read -r -a gio_flags <<< "$(pkg-config --cflags --libs gio-2.0)"
gcc -Werror -o "$WORK_DIR/ottrin-gio-smoke" "$WORK_DIR/gio-smoke.c" "${gio_flags[@]}"

make_stage() {
  local name="$1"
  local executable="${2:-$WORK_DIR/ottrin-gio-smoke}"
  local stage="$WORK_DIR/$name"
  mkdir -p "$stage/libexec/ottrin" "$stage/notices"
  install -m 0755 "$executable" "$stage/libexec/ottrin/ottrin"
  bundle_linux_runtime "$executable" "$stage" "$stage/notices"
  printf '%s\n' "$stage"
}

verify_stage() {
  local stage="$1"
  verify_linux_runtime_closure "$2" "$stage" "$stage/libexec/ottrin/ottrin" "$stage/lib" \
    "$stage/notices/BUNDLED_RUNTIME_MANIFEST.txt" "$stage/notices"
}

expect_reject() {
  local description="$1"
  local expected_diagnostic="$2"
  shift 2
  local stderr_file="$WORK_DIR/reject-stderr"
  if "$@" >/dev/null 2>"$stderr_file"; then
    printf 'runtime closure verifier accepted %s\n' "$description" >&2
    exit 1
  fi
  if ! grep -Fq -- "$expected_diagnostic" "$stderr_file"; then
    printf 'runtime closure verifier rejected %s for the wrong reason; expected diagnostic: %s\n' \
      "$description" "$expected_diagnostic" >&2
    cat "$stderr_file" >&2
    exit 1
  fi
}

set_manifest_version() {
  local stage="$1"
  local version="$2"
  local package
  package="$(awk -F '\t' 'NR == 2 { print $3; exit }' "$stage/notices/BUNDLED_RUNTIME_MANIFEST.txt")"
  [[ -n "$package" ]] || { echo 'unable to choose a real manifest package for version mutation.' >&2; exit 1; }
  awk -F '\t' -v package="$package" -v version="$version" \
    'BEGIN { OFS=FS } NR > 1 && $3 == package { $4=version } { print }' \
    "$stage/notices/BUNDLED_RUNTIME_MANIFEST.txt" > "$stage/manifest"
  mv "$stage/manifest" "$stage/notices/BUNDLED_RUNTIME_MANIFEST.txt"
}

set_first_manifest_field() {
  local stage="$1"
  local field="$2"
  local value="$3"
  awk -F '\t' -v field="$field" -v value="$value" \
    'BEGIN { OFS=FS } NR == 2 { $field=value } { print }' \
    "$stage/notices/BUNDLED_RUNTIME_MANIFEST.txt" > "$stage/manifest"
  mv "$stage/manifest" "$stage/notices/BUNDLED_RUNTIME_MANIFEST.txt"
}

STAGE="$(make_stage positive)"
verify_stage "$STAGE" 'positive disposable GIO closure'
POSITIVE_STAGE="$STAGE"

gcc -Werror -Wl,-rpath,"$WORK_DIR/safe-rpath" -o "$WORK_DIR/ottrin-gio-rpath-smoke" \
  "$WORK_DIR/gio-smoke.c" "${gio_flags[@]}"
STAGE="$(make_stage rpath "$WORK_DIR/ottrin-gio-rpath-smoke")"
expect_reject 'an otherwise valid GIO closure whose executable declares RPATH/RUNPATH' \
  'contains an unsupported ELF RPATH/RUNPATH' verify_stage "$STAGE" 'RPATH closure'

# The smoke has no desktop entries or XDG MIME data in its environment. It is
# intentionally an artifact-level runtime check, not a clean-desktop claim.
mkdir -p "$STAGE/empty-xdg"
env -i LD_LIBRARY_PATH="$STAGE/lib" XDG_DATA_HOME="$STAGE/empty-xdg" XDG_DATA_DIRS="$STAGE/empty-xdg" \
  "$STAGE/libexec/ottrin/ottrin"

# The final verifier must ignore hostile loader state inherited by its caller.
# Run the whole probe in a child so the deliberately invalid variables do not
# make diagnostic helpers noisy; the verifier's own trace is still exercised.
# Positional parameters expand in the child shell, not in this caller.
# shellcheck disable=SC2016
env LD_PRELOAD=/definitely/not/a/runtime-library.so LD_AUDIT=/definitely/not/an/audit-module.so \
  bash -c 'source "$1"; verify_linux_runtime_closure hostile "$2" "$3" "$4" "$5" "$6"' -- \
  "$HELPER" "$POSITIVE_STAGE" "$POSITIVE_STAGE/libexec/ottrin/ottrin" "$POSITIVE_STAGE/lib" \
  "$POSITIVE_STAGE/notices/BUNDLED_RUNTIME_MANIFEST.txt" "$POSITIVE_STAGE/notices" >/dev/null 2>&1 || {
    echo 'runtime closure verifier inherited hostile loader state.' >&2
    exit 1
  }

STAGE="$(make_stage missing-gio-file)"
rm "$STAGE/lib/libgio-2.0.so.0"
expect_reject 'a missing GIO library file' 'runtime library is missing or symlinked: libgio-2.0.so.0' \
  verify_stage "$STAGE" 'missing GIO library'

STAGE="$(make_stage pruned-gio-manifest)"
rm "$STAGE/lib/libgio-2.0.so.0"
sed -i '/^libgio-2\.0\.so\.0\t/d' "$STAGE/notices/BUNDLED_RUNTIME_MANIFEST.txt"
expect_reject 'the reviewer reproduced missing-GIO plus pruned-manifest bypass' \
  'runtime manifest is missing required libgio-2.0.so.0' verify_stage "$STAGE" 'pruned GIO manifest'

STAGE="$(make_stage pruned-gobject-manifest)"
rm "$STAGE/lib/libgobject-2.0.so.0"
sed -i '/^libgobject-2\.0\.so\.0\t/d' "$STAGE/notices/BUNDLED_RUNTIME_MANIFEST.txt"
expect_reject 'a pruned transitive GObject library plus its manifest row' \
  'missing a runtime manifest row for required libgobject-2.0.so.0' verify_stage "$STAGE" 'pruned GObject manifest'

STAGE="$(make_stage pruned-deep-transitive-manifest)"
rm "$STAGE/lib/libffi.so.8"
sed -i '/^libffi\.so\.8\t/d' "$STAGE/notices/BUNDLED_RUNTIME_MANIFEST.txt"
expect_reject 'a pruned deeper transitive library plus its manifest row' \
  'missing a runtime manifest row for required libffi.so.8' verify_stage "$STAGE" 'pruned deep transitive manifest'

STAGE="$(make_stage tampered-library)"
printf 'tampered\n' >> "$STAGE/lib/libgio-2.0.so.0"
expect_reject 'tampered runtime library bytes' 'runtime library digest mismatch: libgio-2.0.so.0' verify_stage "$STAGE" 'tampered library'

STAGE="$(make_stage missing-notice)"
notice="$(awk -F '\t' '$1 == "libgio-2.0.so.0" { print $5; exit }' "$STAGE/notices/BUNDLED_RUNTIME_MANIFEST.txt")"
rm "$STAGE/notices/$notice"
expect_reject 'a missing runtime notice' 'is missing runtime notice:' verify_stage "$STAGE" 'missing notice'

STAGE="$(make_stage tampered-notice)"
notice="$(awk -F '\t' '$1 == "libgio-2.0.so.0" { print $5; exit }' "$STAGE/notices/BUNDLED_RUNTIME_MANIFEST.txt")"
printf 'tampered\n' >> "$STAGE/notices/$notice"
expect_reject 'tampered runtime notice bytes' 'runtime notice digest mismatch:' verify_stage "$STAGE" 'tampered notice'

STAGE="$(make_stage symlink-notice-leaf)"
notice="$(awk -F '\t' '$1 == "libgio-2.0.so.0" { print $5; exit }' "$STAGE/notices/BUNDLED_RUNTIME_MANIFEST.txt")"
mv "$STAGE/notices/$notice" "$WORK_DIR/external-notice-leaf"
ln -s "$WORK_DIR/external-notice-leaf" "$STAGE/notices/$notice"
expect_reject 'an individual runtime notice symlinked outside the notice root' \
  'runtime notice is unsafe:' verify_stage "$STAGE" 'symlinked notice leaf'

STAGE="$(make_stage symlink-library)"
mv "$STAGE/lib/libgio-2.0.so.0" "$STAGE/lib/libgio-2.0.so.0.saved"
ln -s libgio-2.0.so.0.saved "$STAGE/lib/libgio-2.0.so.0"
expect_reject 'a symlinked runtime library' 'runtime library is missing or symlinked: libgio-2.0.so.0' verify_stage "$STAGE" 'symlinked library'

STAGE="$(make_stage symlink-library-root)"
mv "$STAGE/lib" "$WORK_DIR/external-lib-root"
ln -s "$WORK_DIR/external-lib-root" "$STAGE/lib"
expect_reject 'a library directory symlinked outside the artifact root' \
  'is missing a trusted artifact root' verify_stage "$STAGE" 'symlinked library root'

STAGE="$(make_stage symlink-artifact-root)"
ln -s "$STAGE" "$WORK_DIR/symlinked-artifact-root"
expect_reject 'a symlinked artifact root' \
  'is missing a trusted artifact root' \
  verify_linux_runtime_closure 'symlinked artifact root' "$WORK_DIR/symlinked-artifact-root" \
  "$STAGE/libexec/ottrin/ottrin" "$STAGE/lib" \
  "$STAGE/notices/BUNDLED_RUNTIME_MANIFEST.txt" "$STAGE/notices"

STAGE="$(make_stage symlink-notice-root)"
mv "$STAGE/notices" "$WORK_DIR/external-notice-root"
ln -s "$WORK_DIR/external-notice-root" "$STAGE/notices"
expect_reject 'a notice root symlinked outside the artifact root' \
  'is missing a trusted artifact root' verify_stage "$STAGE" 'symlinked notice root'

STAGE="$(make_stage symlink-manifest)"
mv "$STAGE/notices/BUNDLED_RUNTIME_MANIFEST.txt" "$WORK_DIR/external-manifest"
ln -s "$WORK_DIR/external-manifest" "$STAGE/notices/BUNDLED_RUNTIME_MANIFEST.txt"
expect_reject 'a symlinked runtime manifest' 'is missing a trusted artifact root' verify_stage "$STAGE" 'symlinked manifest'

STAGE="$(make_stage symlink-intermediate-parent)"
mv "$STAGE/libexec" "$WORK_DIR/external-libexec"
ln -s "$WORK_DIR/external-libexec" "$STAGE/libexec"
expect_reject 'an executable reached through an intermediate symlink escaping the artifact root' \
  'has an unsafe runtime root' verify_stage "$STAGE" 'symlinked executable parent'

STAGE="$(make_stage executable-outside-root)"
expect_reject 'an executable path outside the artifact root' \
  'has an unsafe runtime root' \
  verify_linux_runtime_closure 'outside executable' "$STAGE" "$WORK_DIR/ottrin-gio-smoke" \
  "$STAGE/lib" "$STAGE/notices/BUNDLED_RUNTIME_MANIFEST.txt" "$STAGE/notices"

STAGE="$(make_stage wrong-soname)"
cp "$STAGE/lib/libglib-2.0.so.0" "$STAGE/lib/libgio-2.0.so.0"
new_sha="$(sha256sum "$STAGE/lib/libgio-2.0.so.0" | awk '{print $1}')"
awk -F '\t' -v sha="$new_sha" 'BEGIN { OFS=FS } $1 == "libgio-2.0.so.0" { $2=sha } { print }' \
  "$STAGE/notices/BUNDLED_RUNTIME_MANIFEST.txt" > "$STAGE/manifest"
mv "$STAGE/manifest" "$STAGE/notices/BUNDLED_RUNTIME_MANIFEST.txt"
expect_reject 'a library with the wrong embedded SONAME' 'runtime library SONAME mismatch: libgio-2.0.so.0' verify_stage "$STAGE" 'wrong SONAME'

STAGE="$(make_stage malformed-manifest)"
printf 'libgio-2.0.so.0\tbroken\n' >> "$STAGE/notices/BUNDLED_RUNTIME_MANIFEST.txt"
expect_reject 'a malformed manifest row' 'has a malformed runtime manifest row' verify_stage "$STAGE" 'malformed manifest'

STAGE="$(make_stage missing-manifest-header)"
sed -i '1d' "$STAGE/notices/BUNDLED_RUNTIME_MANIFEST.txt"
expect_reject 'a manifest missing its required first header' \
  'is missing the unsigned build-record manifest header' verify_stage "$STAGE" 'missing manifest header'

STAGE="$(make_stage malformed-manifest-header)"
sed -i '1c\\# malformed runtime manifest header' "$STAGE/notices/BUNDLED_RUNTIME_MANIFEST.txt"
expect_reject 'a manifest with a malformed first header' \
  'has a malformed runtime manifest header' verify_stage "$STAGE" 'malformed manifest header'

STAGE="$(make_stage repeated-manifest-header)"
sed -i '1p' "$STAGE/notices/BUNDLED_RUNTIME_MANIFEST.txt"
expect_reject 'a manifest with a repeated header' \
  'has a malformed runtime manifest header' verify_stage "$STAGE" 'repeated manifest header'

STAGE="$(make_stage malformed-package)"
set_first_manifest_field "$STAGE" 3 'invalid package'
expect_reject 'a malformed Debian package name' 'has an invalid runtime manifest package' verify_stage "$STAGE" 'malformed package'

STAGE="$(make_stage malformed-version)"
set_manifest_version "$STAGE" ''
expect_reject 'an empty Debian package version' 'has an invalid runtime manifest package version' verify_stage "$STAGE" 'empty version'

STAGE="$(make_stage invalid-debian-version)"
set_manifest_version "$STAGE" '1_bad'
expect_reject 'a Debian version dpkg rejects' 'has an invalid runtime manifest package version' verify_stage "$STAGE" 'invalid Debian version'

STAGE="$(make_stage malformed-debian-epoch)"
set_manifest_version "$STAGE" ':1'
expect_reject 'a Debian version with a malformed epoch' \
  'has an invalid runtime manifest package version' verify_stage "$STAGE" 'malformed Debian epoch'

STAGE="$(make_stage malformed-debian-revision)"
set_manifest_version "$STAGE" '1-'
expect_reject 'a Debian version with an empty revision' \
  'has an invalid runtime manifest package version' verify_stage "$STAGE" 'malformed Debian revision'

STAGE="$(make_stage invalid-debian-character)"
set_manifest_version "$STAGE" '1?2'
expect_reject 'another Debian version dpkg rejects' \
  'has an invalid runtime manifest package version' verify_stage "$STAGE" 'invalid Debian character'

STAGE="$(make_stage valid-debian-version)"
set_manifest_version "$STAGE" '1:2.0~rc1+build-3'
verify_stage "$STAGE" 'valid epoch tilde plus revision Debian version'

STAGE="$(make_stage control-character-version)"
set_manifest_version "$STAGE" $'bad\001version'
expect_reject 'a control character in a Debian package version' \
  'has an invalid runtime manifest package version' verify_stage "$STAGE" 'control-character version'

STAGE="$(make_stage duplicate-manifest)"
duplicate_row="$(sed -n '2p' "$STAGE/notices/BUNDLED_RUNTIME_MANIFEST.txt")"
printf '%s\n' "$duplicate_row" >> "$STAGE/notices/BUNDLED_RUNTIME_MANIFEST.txt"
expect_reject 'a duplicate manifest SONAME' 'has duplicate runtime manifest SONAME:' verify_stage "$STAGE" 'duplicate manifest'

STAGE="$(make_stage traversal-manifest)"
set_first_manifest_field "$STAGE" 5 '../outside'
expect_reject 'a traversal manifest path' 'has an unsafe runtime notice path' verify_stage "$STAGE" 'traversal manifest'

printf 'Verified strict Linux GIO runtime closure behavior.\n'
