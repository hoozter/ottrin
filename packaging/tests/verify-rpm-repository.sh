#!/usr/bin/env bash
set -euo pipefail

root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
input="${1:-$root/dist/packages/ottrin-0.1.0-1.x86_64.rpm}"
fixture="$(mktemp -d)"
export GNUPGHOME="$fixture/gnupg"
mkdir -m 0700 "$GNUPGHOME"
cleanup() {
  gpgconf --kill all
  rm -rf -- "$fixture"
}
trap cleanup EXIT
gpg --batch --passphrase '' --quick-generate-key 'Ottrin RPM archive test' rsa2048 sign 1d
export OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT
OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT="$(gpg --batch --with-colons --list-keys | awk -F: '$1=="fpr" {print $10; exit}')"

package="$fixture/ottrin.rpm"
cp -- "$input" "$package"
rpmsign --delsign "$package"
builder="$root/packaging/linux/build-rpm-repository.sh"
if "$builder" "$fixture/rejected" "$package"; then
  printf 'Unsigned RPM was accepted.\n' >&2
  exit 1
fi
test ! -e "$fixture/rejected"
"$root/packaging/linux/sign-rpm-packages.sh" "$package"
archive="$fixture/archive"
"$builder" "$archive" "$package"
archived="$(find "$archive/Packages" -type f -name '*.rpm' -print)"
cmp "$package" "$archived"
mkdir "$fixture/rpmdb"
rpmkeys --dbpath "$fixture/rpmdb" --import "$archive/ottrin-rpm-key.asc"
rpmkeys --dbpath "$fixture/rpmdb" --checksig --verbose "$archived" | grep -E 'Signature, key ID [[:xdigit:]]+: OK$'
gpg --batch --verify "$archive/repodata/repomd.xml.asc" "$archive/repodata/repomd.xml"
python3 - "$archive" <<'PY'
from pathlib import Path
import gzip
import hashlib
import sys
import xml.etree.ElementTree as ET

root = Path(sys.argv[1])
metadata = ET.parse(root / "repodata/repomd.xml")
for item in metadata.findall("{*}data"):
    checksum = item.find("{*}checksum")
    path = root / item.find("{*}location").attrib["href"]
    assert hashlib.new(checksum.attrib["type"], path.read_bytes()).hexdigest() == checksum.text
    if item.attrib["type"] == "primary":
        with gzip.open(path) as stream:
            index = ET.parse(stream)
        packages = index.findall("{*}package")
        assert len(packages) == 1 and packages[0].find("{*}name").text == "ottrin"
        package = packages[0]
        payload = root / package.find("{*}location").attrib["href"]
        digest = package.find("{*}checksum")
        assert hashlib.new(digest.attrib["type"], payload.read_bytes()).hexdigest() == digest.text
PY
printf '\n' >> "$archive/repodata/repomd.xml"
if gpg --batch --verify "$archive/repodata/repomd.xml.asc" "$archive/repodata/repomd.xml"; then
  printf 'Altered RPM metadata passed signature verification.\n' >&2
  exit 1
fi
printf 'RPM signatures and repository checksums verified. Altered metadata was rejected.\n'
