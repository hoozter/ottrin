# Sourced by repository tests after setting root to the checkout.
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
  --quick-generate-key 'Ottrin disposable repository test' ed25519 cert 1d
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
