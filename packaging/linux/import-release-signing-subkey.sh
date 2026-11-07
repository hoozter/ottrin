#!/usr/bin/env bash
# Read the CI signing export from stdin into the workflow's isolated keyring.
set -euo pipefail

: "${GNUPGHOME:?Use an isolated GNUPGHOME for release signing}"
fingerprint="${OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT:-}"
fingerprint="${fingerprint^^}"
if [[ ! "$fingerprint" =~ ^[0-9A-F]{40}$ ]]; then
  printf 'Set the approved Linux primary-key fingerprint before importing the signing subkey.\n' >&2
  exit 2
fi

gpg --batch --import
if ! gpg --batch --with-colons --list-secret-keys | awk -F: -v expected="$fingerprint" '
  $1 == "sec" { primary_count++; stub = ($15 == "#"); primary_record = 1 }
  $1 == "ssb" && $15 != "#" { subkey_count++; signing = ($15 == "+" && $12 == "s") }
  $1 == "fpr" && primary_record { approved = (toupper($10) == expected); primary_record = 0 }
  END { exit !(primary_count == 1 && approved && stub && subkey_count == 1 && signing) }
'; then
  printf 'Linux CI needs exactly one signing subkey for the approved identity, with the primary secret held offline.\n' >&2
  exit 1
fi
