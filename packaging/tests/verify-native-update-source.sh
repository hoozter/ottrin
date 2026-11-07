#!/usr/bin/env bash
# Build and inspect native package ownership using a disposable public key.
set -euo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
fixture="$(mktemp -d)"
export GNUPGHOME="$fixture/gnupg"
mkdir -m 0700 "$GNUPGHOME"
cleanup() {
  gpgconf --kill all
  rm -rf -- "$fixture"
}
trap cleanup EXIT
gpg --batch --passphrase '' --quick-generate-key 'Ottrin update source test' rsa2048 sign 1d
export OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT
OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT="$(gpg --batch --with-colons --list-keys | awk -F: '$1=="fpr" {print $10; exit}')"
export OTTRIN_LINUX_REPOSITORY_URL=https://updates.example.invalid
export OTTRIN_LINUX_REPOSITORY_KEY="$fixture/public.asc"
gpg --batch --armor --export "$OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT" > "$OTTRIN_LINUX_REPOSITORY_KEY"
export OTTRIN_DIST_DIR="$fixture/packages"
export OTTRIN_SKIP_BUILD=1
"$root/packaging/linux/package-deb.sh"
"$root/packaging/linux/package-rpm.sh"

dpkg-deb --raw-extract "$OTTRIN_DIST_DIR"/ottrin_*_amd64.deb "$fixture/deb"
grep -Fx 'URIs: https://updates.example.invalid/apt' "$fixture/deb/etc/apt/sources.list.d/ottrin.sources"
grep -Fx 'Signed-By:' "$fixture/deb/etc/apt/sources.list.d/ottrin.sources"
grep -Fx '/etc/apt/sources.list.d/ottrin.sources' "$fixture/deb/DEBIAN/conffiles"
grep -Fx '/etc/apt/preferences.d/ottrin.pref' "$fixture/deb/DEBIAN/conffiles"
grep -Fx 'Pin-Priority: -1' "$fixture/deb/etc/apt/preferences.d/ottrin.pref"
sed -n '/^Signed-By:$/,$ { /^ / { s/^ //; s/^\.$//; p; } }' \
  "$fixture/deb/etc/apt/sources.list.d/ottrin.sources" > "$fixture/embedded.asc"
gpg --batch --show-keys "$fixture/embedded.asc" >/dev/null

rpm_file=("$OTTRIN_DIST_DIR"/ottrin-*.x86_64.rpm)
rpm -qp --queryformat '[%{FILENAMES} %{FILEFLAGS}\n]' "${rpm_file[0]}" |
  awk '$1=="/etc/yum.repos.d/ottrin.repo" && ($2 % 32) == 17 { valid=1 } END {exit !valid}'
mkdir "$fixture/rpm"
(cd "$fixture/rpm" && rpm2cpio "${rpm_file[0]}" | cpio -idm --quiet)
repo="$fixture/rpm/etc/yum.repos.d/ottrin.repo"
for setting in baseurl=https://updates.example.invalid/rpm gpgcheck=1 repo_gpgcheck=1 includepkgs=ottrin; do
  grep -Fx "$setting" "$repo"
done
gpg --batch --show-keys "$fixture/rpm/usr/share/ottrin/ottrin-rpm-key.asc" >/dev/null

unset OTTRIN_LINUX_REPOSITORY_URL OTTRIN_LINUX_REPOSITORY_KEY
mkdir "$fixture/opt-out"
"$root/packaging/linux/stage-update-source.sh" deb "$fixture/opt-out"
"$root/packaging/linux/stage-update-source.sh" rpm "$fixture/opt-out"
test -z "$(ls -A "$fixture/opt-out")"
printf 'Native packages own their scoped update configuration and public keys.\n'
