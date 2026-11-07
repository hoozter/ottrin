#!/usr/bin/env python3
"""Publish one verified stable repository snapshot to R2."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
from tempfile import TemporaryDirectory
from urllib.parse import quote, urlsplit
from urllib.request import Request, urlopen

NO_CACHE = "no-store"
IMMUTABLE = "public, max-age=31536000, immutable"
POINTERS = (
    "rpm/repodata/repomd.xml.asc",
    "rpm/repodata/repomd.xml",
    "apt/dists/stable/main/binary-amd64/Packages",
    "apt/dists/stable/main/binary-amd64/Packages.gz",
    "apt/dists/stable/Release",
    "apt/dists/stable/InRelease",
)
KEYS = ("apt/ottrin-archive-keyring.gpg", "rpm/ottrin-rpm-key.asc")
RECEIPT = "linux-release.json"


def version(release):
    tag = re.fullmatch(r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", release["tag_name"])
    if not tag or type(release["id"]) is not int or release["id"] <= 0 or not release["published_at"]:
        raise ValueError("Invalid stable release identity")
    return tuple(map(int, tag.groups()))


def digest(stream):
    return hashlib.file_digest(stream, "sha256").hexdigest()


class Repository:
    def __init__(self, endpoint, bucket, public_url):
        host = urlsplit(public_url)
        if host.scheme != "https" or not host.hostname or host.netloc != host.hostname or host.path or host.query or host.fragment:
            raise ValueError("The public repository URL must be an HTTPS origin")
        self.aws = ["aws", "--endpoint-url", endpoint, "--output", "json", "s3api"]
        self.bucket = bucket
        self.public_url = public_url

    def s3(self, operation, key, *args, missing_ok=False):
        result = subprocess.run(
            [*self.aws, operation, "--bucket", self.bucket, "--key", key, *args],
            text=True, capture_output=True, check=False,
        )
        if result.returncode:
            if missing_ok and re.search(r"\((404|NoSuchKey|NotFound)\)", result.stderr):
                return None
            raise ValueError(f"R2 {operation} failed for {key}: {result.stderr.strip()}")
        return json.loads(result.stdout or "{}")

    def verify(self, key, path, cache_control):
        request = Request(f"{self.public_url}/{quote(key)}", headers={"Accept-Encoding": "identity"})
        with path.open("rb") as source, urlopen(request, timeout=60) as response:
            if (response.geturl() != request.full_url
                    or response.headers.get("Cache-Control") != cache_control
                    or digest(response) != digest(source)):
                raise ValueError(f"Public repository verification failed for {key}")

    def upload(self, key, path, cache_control, *condition):
        result = self.s3("put-object", key, "--body", str(path), "--cache-control", cache_control, *condition)
        self.verify(key, path, cache_control)
        return result

    def immutable(self, key, path, cache_control):
        if self.s3("head-object", key, missing_ok=True) is None:
            self.upload(key, path, cache_control, "--if-none-match", "*")
        else:
            self.verify(key, path, cache_control)


def verify_latest(release, github_repository):
    latest = json.loads(subprocess.check_output(
        ["gh", "api", f"repos/{github_repository}/releases/latest"], text=True,
    ))
    if (latest.get("draft") is not False or latest.get("prerelease") is not False
            or any(latest.get(field) != release[field] for field in ("id", "tag_name", "published_at"))):
        raise ValueError("Snapshot no longer matches the current published stable GitHub release")


def publish(snapshot, repository, github_repository, work):
    release = json.loads((snapshot / "release.json").read_text())
    candidate = version(release)
    files = {path.relative_to(snapshot).as_posix(): path
             for path in snapshot.rglob("*") if path.is_file()}
    files.pop("release.json")
    payloads = set(files).difference(POINTERS, KEYS)
    prefixes = ("apt/pool/main/o/ottrin/", "apt/dists/stable/main/binary-amd64/by-hash/",
                "rpm/Packages/", "rpm/repodata/")
    if (not all(key in files for key in (*POINTERS, *KEYS))
            or not all(any(key.startswith(prefix) for key in payloads) for prefix in prefixes)
            or any(not key.startswith(prefixes) for key in payloads)
            or any(path.is_symlink() or not path.stat().st_size for path in files.values())):
        raise ValueError("Repository snapshot is incomplete or contains unexpected files")

    previous_path = work / "previous.json"
    previous_object = repository.s3("get-object", RECEIPT, str(previous_path), missing_ok=True)
    already_published = False
    if previous_object is None:
        condition = ("--if-none-match", "*")
    else:
        previous = json.loads(previous_path.read_text())
        published = version(previous)
        if previous["state"] not in {"publishing", "published"}:
            raise ValueError("Invalid publication receipt state")
        same_release = all(release[field] == previous[field] for field in ("id", "tag_name", "published_at"))
        if candidate < published or (candidate == published and not same_release):
            raise ValueError("Refusing to replace a newer or different stable release")
        already_published = same_release and previous["state"] == "published"
        condition = ("--if-match", previous_object["ETag"])

    verify_latest(release, github_repository)
    if already_published:
        # A rebuilt snapshot can have different metadata timestamps. Verify
        # the versioned package bytes without uploading another set of indexes.
        for key in sorted(key for key in payloads if key.startswith(("apt/pool/", "rpm/Packages/"))):
            repository.verify(key, files[key], IMMUTABLE)
        for key in KEYS:
            repository.verify(key, files[key], NO_CACHE)
        print(f"{release['tag_name']} is already published. Stable metadata was left in place.")
        return

    for key in sorted(payloads):
        repository.immutable(key, files[key], IMMUTABLE)
    for key in KEYS:
        repository.immutable(key, files[key], NO_CACHE)

    verify_latest(release, github_repository)

    # The workflow serializes publishers. Reserve the version before switching
    # any metadata so even an interrupted run cannot allow a downgrade.
    receipt = work / RECEIPT
    receipt.write_text(json.dumps({**release, "state": "publishing"}) + "\n")
    reservation = repository.upload(RECEIPT, receipt, NO_CACHE, *condition)
    for key in POINTERS:
        repository.upload(key, files[key], NO_CACHE)
    receipt.write_text(json.dumps({**release, "state": "published"}) + "\n")
    repository.upload(RECEIPT, receipt, NO_CACHE, "--if-match", reservation["ETag"])


if __name__ == "__main__":
    try:
        if len(sys.argv) != 2:
            raise ValueError("Usage: publish-release-repositories.py SNAPSHOT")
        account = os.environ["R2_ACCOUNT_ID"]
        if not re.fullmatch(r"[a-f0-9]{32}", account):
            raise ValueError("Invalid Cloudflare account ID")
        repository = Repository(
            f"https://{account}.r2.cloudflarestorage.com",
            os.environ["R2_BUCKET"], os.environ["LINUX_REPOSITORY_PUBLIC_ORIGIN"],
        )
        with TemporaryDirectory(prefix="ottrin-publish-") as temporary:
            publish(Path(sys.argv[1]), repository, os.environ["GITHUB_REPOSITORY"], Path(temporary))
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        sys.exit(f"Repository publication failed: {error}")
