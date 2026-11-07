#!/usr/bin/env bash
# Exercise the publication gate with a fake GitHub API and real signed assets.
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
export GITHUB_REPOSITORY=fixture/ottrin
export GITHUB_FIXTURE="$fixture"
mkdir "$fixture/bin" "$fixture/assets"
cat > "$fixture/bin/gh" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
case "$1:$2" in
  api:repos/fixture/ottrin/releases/tags/v0.1.0)
    cat "$GITHUB_FIXTURE/release.json"
    ;;
  api:repos/fixture/ottrin/releases/latest)
    cat "$GITHUB_FIXTURE/latest"
    ;;
  release:download)
    shift 2
    test "$1" = v0.1.0
    shift 3
    test "$1" = --dir
    out="$2"
    shift 2
    touch "$GITHUB_FIXTURE/download-attempted"
    while (( $# )); do
      test "$1" = --pattern
      cp -- "$GITHUB_FIXTURE/assets/$2" "$out/$2"
      shift 2
    done
    ;;
  *)
    exit 90
    ;;
esac
MOCK
chmod +x "$fixture/bin/gh"
export PATH="$fixture/bin:$PATH"
gate="$root/packaging/linux/prepare-release-repositories.sh"
set_release() {
  printf '{"id":101,"tag_name":"v0.1.0","draft":%s,"prerelease":%s,"published_at":%s,"assets":[{"name":"ottrin-0.1.0-1.x86_64.rpm"}]}\n' \
    "$1" "$2" "$3" > "$fixture/release.json"
  printf '%s\n' "$4" > "$fixture/latest"
}
reject() {
  if "$gate" "$1" "$fixture/rejected"; then
    printf 'Publication gate accepted an ineligible release.\n' >&2
    exit 1
  fi
  test ! -e "$fixture/rejected"
}
set_release true false null 101
reject v0.1.0
set_release false true '"2026-11-01T00:00:00Z"' 101
reject v0.1.0
set_release false false '"2026-11-01T00:00:00Z"' 102
reject v0.1.0
reject v0.1.0-rc.1
test ! -e "$fixture/download-attempted"

gpg --batch --passphrase '' --quick-generate-key 'Ottrin release gate test' rsa2048 sign 1d
export OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT
OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT="$(gpg --batch --with-colons --list-keys | awk -F: '$1=="fpr" {print $10; exit}')"
(
  cd "$fixture/assets"
  cp -- "$root/dist/packages/ottrin_0.1.0_amd64.deb" "$root/dist/packages/ottrin-0.1.0-1.x86_64.rpm" .
  "$root/packaging/linux/sign-rpm-packages.sh" ottrin-0.1.0-1.x86_64.rpm
  for package in *.deb *.rpm; do
    sha256sum "$package" > "$package.sha256"
  done
  "$root/packaging/linux/sign-release-artifacts.sh" ./*.deb ./*.rpm
  gpg --batch --armor --export "$OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT" > Ottrin-linux-release-key.asc
)
set_release false false '"2026-11-01T00:00:00Z"' 101
"$gate" v0.1.0 "$fixture/staged"
test -s "$fixture/staged/apt/dists/stable/InRelease"
test -s "$fixture/staged/rpm/repodata/repomd.xml.asc"
jq -e '.id == 101 and .tag_name == "v0.1.0"' "$fixture/staged/release.json"
printf '\n' >> "$fixture/assets/ottrin_0.1.0_amd64.deb"
reject v0.1.0
printf 'Published stable assets staged. Draft, prerelease, superseded and altered releases were rejected.\n'
