#!/usr/bin/env python3
"""Exercise detached release signatures with an ephemeral offline primary and CI subkey."""

from __future__ import annotations

from contextlib import ExitStack
import os
from pathlib import Path
import subprocess
import tempfile
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
IMPORTER = ROOT / "packaging/linux/import-release-signing-subkey.sh"
SIGNER = ROOT / "packaging/linux/sign-release-artifacts.sh"
VERIFIER = ROOT / "packaging/linux/verify-release-signatures.sh"


def run(
    *args: object, env: dict[str, str], **kwargs: Any
) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run([str(arg) for arg in args], env=env, check=True, **kwargs)


with ExitStack() as stack:
    work = Path(stack.enter_context(tempfile.TemporaryDirectory(prefix="ottrin-signing-test-")))

    def keyring(name: str) -> dict[str, str]:
        home = work / name
        home.mkdir(mode=0o700)
        env = os.environ | {"GNUPGHOME": str(home)}
        stack.callback(run, "gpgconf", "--kill", "all", env=env)
        return env

    primary_env = keyring("offline-primary")
    signing_env = keyring("ci-signer")
    rejected_env = keyring("rejected-primary")
    run(
        "gpg",
        "--batch",
        "--passphrase",
        "",
        "--quick-generate-key",
        "Ottrin Test Signing <test@ottrin.invalid>",
        "ed25519",
        "cert",
        "0",
        env=primary_env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    listing = subprocess.check_output(
        ["gpg", "--batch", "--with-colons", "--list-secret-keys"],
        env=primary_env,
        text=True,
    )
    fingerprint = next(
        line.split(":")[9] for line in listing.splitlines() if line.startswith("fpr:")
    )

    run(
        "gpg", "--batch", "--passphrase", "", "--quick-add-key",
        fingerprint, "ed25519", "sign", "0", env=primary_env,
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    listing = subprocess.check_output(
        ["gpg", "--batch", "--with-colons", "--list-secret-keys", fingerprint],
        env=primary_env, text=True,
    )
    primary, subkey = [
        line.split(":")[9] for line in listing.splitlines() if line.startswith("fpr:")
    ]
    assert primary == fingerprint
    for env in (signing_env, rejected_env):
        env["OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT"] = fingerprint

    full_export = run(
        "gpg", "--batch", "--export-secret-keys", fingerprint, env=primary_env,
        stdout=subprocess.PIPE,
    ).stdout
    rejected = subprocess.run(
        [str(IMPORTER)], input=full_export, env=rejected_env, capture_output=True,
    )
    if rejected.returncode != 1 or b"primary secret held offline" not in rejected.stderr:
        raise SystemExit("CI importer did not reject the full private key")

    subkey_export = run(
        "gpg", "--batch", "--export-secret-subkeys", f"{subkey}!", env=primary_env,
        stdout=subprocess.PIPE,
    ).stdout
    run(IMPORTER, input=subkey_export, env=signing_env)
    artifact = work / "artifact.bin"
    artifact.write_text("signed payload\n", encoding="utf-8")
    run(SIGNER, artifact, env=signing_env)

    public_key = work / "public.asc"
    public_key.write_bytes(
        subprocess.check_output(
            ["gpg", "--batch", "--armor", "--export", fingerprint],
            env=primary_env,
        )
    )
    verification_tmp = work / "verification-tmp"
    verification_tmp.mkdir()
    verification_env = os.environ | {
        "TMPDIR": str(verification_tmp),
        "OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT": fingerprint,
    }
    verification_env.pop("GNUPGHOME", None)
    run(VERIFIER, public_key, artifact, env=verification_env)
    retained_verification_state = list(verification_tmp.iterdir())
    if retained_verification_state:
        raise SystemExit(
            f"verifier retained private GPG state: {retained_verification_state}"
        )

    artifact.write_text("tampered payload\n", encoding="utf-8")
    tampered = subprocess.run(
        [str(VERIFIER), str(public_key), str(artifact)],
        env=verification_env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    if tampered.returncode == 0:
        raise SystemExit("tampered artifact unexpectedly verified")

print("Verified subkey-only CI signing, full-key rejection and artifact tamper rejection.")
