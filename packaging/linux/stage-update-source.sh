#!/usr/bin/env bash
# Install only package-owned files into the native package staging root.
set -euo pipefail

format="$1"
stage="$2"
url="${OTTRIN_LINUX_REPOSITORY_URL:-}"
key="${OTTRIN_LINUX_REPOSITORY_KEY:-}"
if [[ -z "$url" && -z "$key" ]]; then
  exit 0
fi
if [[ ! "$url" =~ ^https://([a-zA-Z0-9.-]+)(:[0-9]+)?(/[a-zA-Z0-9._/-]+)?$ || ! -s "$key" ]]; then
  printf 'Native updates require an HTTPS repository URL and a public key file.\n' >&2
  exit 2
fi
host="${BASH_REMATCH[1]}"
url="${url%/}"
fingerprint="${OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT:-}"
fingerprint="${fingerprint^^}"
key_home="$(mktemp -d)"
cleanup() {
  GNUPGHOME="$key_home" gpgconf --kill all
  rm -rf -- "$key_home"
}
trap cleanup EXIT
primary="$(gpg --batch --homedir "$key_home" --with-colons --show-keys "$key" |
  awk -F: '$1=="pub" {primary=1} $1=="fpr" && primary {print toupper($10); primary=0}')"
if [[ ! "$fingerprint" =~ ^[0-9A-F]{40}$ || "$primary" != "$fingerprint" ]]; then
  printf 'The update key must contain exactly the approved primary identity.\n' >&2
  exit 2
fi
# Export the public certificate through GnuPG so the staged file cannot
# accidentally contain a private key or unrelated keyring records.
if grep -q -- 'BEGIN PGP PRIVATE KEY BLOCK' "$key"; then
  printf 'Use a public key export for package staging.\n' >&2
  exit 2
fi
gpg --batch --homedir "$key_home" --import-options import-minimal --import "$key"

case "$format" in
  deb)
    mkdir -p "$stage/etc/apt/sources.list.d" "$stage/etc/apt/preferences.d"
    cat > "$stage/etc/apt/sources.list.d/ottrin.sources" <<SOURCE
Types: deb
URIs: $url/apt
Suites: stable
Components: main
Architectures: amd64
Signed-By:
SOURCE
    # dpkg remove keeps conffiles until purge. Keep the scoped key in the
    # retained source so APT can still authenticate it after removal.
    gpg --batch --homedir "$key_home" --armor --export "$fingerprint" |
      sed -e 's/^$/./' -e 's/^/ /' >> "$stage/etc/apt/sources.list.d/ottrin.sources"
    cat > "$stage/etc/apt/preferences.d/ottrin.pref" <<PREFERENCES
Package: ottrin
Pin: origin "$host"
Pin-Priority: 500

Package: *
Pin: origin "$host"
Pin-Priority: -1
PREFERENCES
    ;;
  rpm)
    mkdir -p "$stage/etc/yum.repos.d" "$stage/usr/share/ottrin"
    gpg --batch --homedir "$key_home" --armor --export "$fingerprint" > "$stage/usr/share/ottrin/ottrin-rpm-key.asc"
    cat > "$stage/etc/yum.repos.d/ottrin.repo" <<REPO
[ottrin]
name=Ottrin stable updates
baseurl=$url/rpm
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=file:///usr/share/ottrin/ottrin-rpm-key.asc
includepkgs=ottrin
REPO
    ;;
  *)
    printf 'Unknown native package format: %s\n' "$format" >&2
    exit 2
    ;;
esac
