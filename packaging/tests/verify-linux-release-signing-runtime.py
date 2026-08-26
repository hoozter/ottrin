#!/usr/bin/env python3
"""Exercise detached release signatures with an ephemeral GPG identity."""

from __future__ import annotations

import os
from pathlib import Path
import subprocess
import tempfile
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
SIGNER = ROOT / "packaging/linux/sign-release-artifacts.sh"
VERIFIER = ROOT / "packaging/linux/verify-release-signatures.sh"


def run(
    *args: object, env: dict[str, str], **kwargs: Any
) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run([str(arg) for arg in args], env=env, check=True, **kwargs)


with tempfile.TemporaryDirectory(prefix="ottrin-signing-test-") as temporary:
    work = Path(temporary)
    signing_home = work / "signing-gnupg"
    signing_home.mkdir(mode=0o700)
    signing_env = os.environ | {"GNUPGHOME": str(signing_home)}
    run(
        "gpg",
        "--batch",
        "--passphrase",
        "",
        "--quick-generate-key",
        "Ottrin Test Signing <test@ottrin.invalid>",
        "rsa2048",
        "sign",
        "0",
        env=signing_env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    listing = subprocess.check_output(
        ["gpg", "--batch", "--with-colons", "--list-secret-keys"],
        env=signing_env,
        text=True,
    )
    fingerprint = next(
        line.split(":")[9] for line in listing.splitlines() if line.startswith("fpr:")
    )

    artifact = work / "artifact.bin"
    artifact.write_text("signed payload\n", encoding="utf-8")
    signing_env["OTTRIN_LINUX_SIGNING_KEY_FINGERPRINT"] = fingerprint
    run(SIGNER, artifact, env=signing_env)

    public_key = work / "public.asc"
    public_key.write_bytes(
        subprocess.check_output(
            ["gpg", "--batch", "--armor", "--export", fingerprint],
            env=signing_env,
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

print("Verified Linux release signing and tamper rejection.")
