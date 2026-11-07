#!/usr/bin/env bash
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
gpg --batch --passphrase '' --quick-generate-key 'Ottrin archive test' rsa2048 sign 1d
export OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT
OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT="$(gpg --batch --with-colons --list-keys | awk -F: '$1=="fpr" {print $10; exit}')"

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
cat > "$fixture/ottrin.sources" <<SOURCES
Types: deb
URIs: file:$archive
Suites: stable
Components: main
Architectures: amd64
Signed-By: $archive/ottrin-archive-keyring.gpg
SOURCES
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
