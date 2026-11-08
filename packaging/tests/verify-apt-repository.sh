#!/usr/bin/env bash
set -euo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
fixture="$(mktemp -d)"
primary_home="$fixture/primary"
export GNUPGHOME="$fixture/ci"
mkdir -m 0700 "$primary_home" "$GNUPGHOME"
cleanup() {
  GNUPGHOME="$primary_home" gpgconf --kill all
  gpgconf --kill all
  rm -rf -- "$fixture"
}
trap cleanup EXIT
gpg --homedir "$primary_home" --batch --passphrase '' \
  --quick-generate-key 'Ottrin archive test' ed25519 cert 1d
export OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT
OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT="$(
  gpg --homedir "$primary_home" --batch --with-colons --list-keys |
    awk -F: '$1=="fpr" && !found {print $10; found=1}'
)"
gpg --homedir "$primary_home" --batch --passphrase '' \
  --quick-add-key "$OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT" ed25519 sign 1d
signing_subkey="$(
  gpg --homedir "$primary_home" --batch --with-colons --list-keys |
    awk -F: '$1=="sub" {subkey=1} $1=="fpr" && subkey {print $10; subkey=0}'
)"
gpg --homedir "$primary_home" --batch --export-secret-subkeys "${signing_subkey}!" |
  "$root/packaging/linux/import-release-signing-subkey.sh"

for version in 1.0 1.1; do
  mkdir -p "$fixture/package/DEBIAN"
  cat > "$fixture/package/DEBIAN/control" <<CONTROL
Package: ottrin
Version: $version
Architecture: amd64
Maintainer: Archive fixture <fixture@example.invalid>
Description: Disposable repository verification package
CONTROL
  dpkg-deb --root-owner-group --build "$fixture/package" "$fixture/ottrin_$version.deb"
done
archive="$fixture/archive"
builder="$root/packaging/linux/build-apt-repository.sh"
"$builder" "$archive" "$fixture/ottrin_1.0.deb" "$fixture/ottrin_1.1.deb"
if "$builder" "$fixture/rejected" "$fixture/ottrin_1.0.deb" "$fixture/ottrin_1.0.deb"; then
  printf 'Duplicate package versions must be rejected.\n' >&2
  exit 1
fi
test ! -e "$fixture/rejected"

mkdir -p "$fixture/lists/partial" "$fixture/empty"
export APT_CONFIG="$fixture/apt.conf"
printf 'Dir::Etc::parts "%s";\nDir::Etc::main "/dev/null";\n' "$fixture/empty" > "$APT_CONFIG"
printf 'Package: ottrin\nStatus: install ok installed\nArchitecture: amd64\nVersion: 1.0\nDescription: installed fixture\n' > "$fixture/status"
OTTRIN_LINUX_REPOSITORY_URL=https://updates.example.invalid \
OTTRIN_LINUX_REPOSITORY_KEY="$archive/ottrin-archive-keyring.gpg" \
  "$root/packaging/linux/stage-update-source.sh" deb "$fixture/config"
sed "s|^URIs: .*|URIs: file:$archive|" \
  "$fixture/config/etc/apt/sources.list.d/ottrin.sources" > "$fixture/ottrin.sources"
apt_options=(
  -o "Dir::Etc::sourcelist=$fixture/ottrin.sources"
  -o "Dir::Etc::sourceparts=$fixture/empty"
  -o "Dir::Etc::preferences=/dev/null"
  -o "Dir::Etc::preferencesparts=$fixture/empty"
  -o "Dir::State::lists=$fixture/lists"
  -o "Dir::State::status=$fixture/status"
  -o "Dir::Cache::pkgcache="
  -o "Dir::Cache::srcpkgcache="
  -o "APT::Sandbox::User=$(id -un)"
  -o "APT::Update::Error-Mode=any"
)
apt-get "${apt_options[@]}" update
apt-cache "${apt_options[@]}" policy ottrin | grep -F 'Candidate: 1.1'
(cd "$fixture" && apt-get "${apt_options[@]}" download ottrin=1.1)
cmp "$fixture/ottrin_1.1.deb" "$fixture/ottrin_1.1_amd64.deb"

sed -i 's/^Origin: Ottrin$/Origin: altered/' "$archive/dists/stable/InRelease"
if apt-get "${apt_options[@]}" update > "$fixture/tampered.log" 2>&1; then
  printf 'APT unexpectedly accepted altered InRelease metadata.\n' >&2
  exit 1
fi
grep -Eq 'BADSIG|invalid signature|signatures were invalid' "$fixture/tampered.log"
printf 'APT accepted the signed archive, detected and downloaded the newer package, and refused altered metadata.\n'

# Exercise the real dpkg conffile behavior without the application's service hooks.
install_root="$fixture/install-root"
mkdir -p "$install_root/var/lib/dpkg" "$fixture/config/DEBIAN"
touch "$install_root/var/lib/dpkg/status"
printf '%s\n' /etc/apt/sources.list.d/ottrin.sources /etc/apt/preferences.d/ottrin.pref \
  > "$fixture/config/DEBIAN/conffiles"
for version in 1.0 1.1; do
  dpkg-deb -f "$fixture/ottrin_$version.deb" > "$fixture/config/DEBIAN/control"
  dpkg-deb --root-owner-group --build "$fixture/config" "$fixture/config-$version.deb"
done
dpkg_fixture=(dpkg --root="$install_root" --log="$fixture/dpkg.log" --force-not-root)
source_file="$install_root/etc/apt/sources.list.d/ottrin.sources"
"${dpkg_fixture[@]}" --install "$fixture/config-1.0.deb"
printf 'Enabled: no\n' >> "$source_file"
"${dpkg_fixture[@]}" --install "$fixture/config-1.1.deb"
grep -Fx 'Enabled: no' "$source_file"
"${dpkg_fixture[@]}" --remove ottrin
grep -Fx 'Enabled: no' "$source_file"
"${dpkg_fixture[@]}" --purge ottrin
test ! -e "$source_file"
test ! -e "$install_root/etc/apt/preferences.d/ottrin.pref"
printf 'dpkg preserved the disabled source across upgrade and removal, then removed it on purge.\n'
