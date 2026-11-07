#!/usr/bin/env python3
"""Exercise a stable metadata switch without credentials or network access."""
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
from tempfile import TemporaryDirectory
from unittest.mock import patch

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location(
    "publisher", Path(__file__).resolve().parents[1] / "linux/publish-release-repositories.py",
)
publisher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publisher)


class TestBucket(publisher.Repository):
    def __init__(self):
        self.objects = {}
        self.writes = []
        self.interrupt = None

    def s3(self, operation, key, *args, missing_ok=False):
        existing = self.objects.get(key)
        if operation in {"get-object", "head-object"}:
            if existing is None:
                assert missing_ok
                return None
            if operation == "get-object":
                Path(args[0]).write_bytes(existing[0])
            return {"ETag": hashlib.sha256(existing[0]).hexdigest()}

        assert operation == "put-object"
        options = dict(zip(args[::2], args[1::2], strict=True))
        if options.get("--if-none-match") == "*":
            assert existing is None
        if "--if-match" in options:
            assert options["--if-match"] == hashlib.sha256(existing[0]).hexdigest()
        if key == self.interrupt:
            raise OSError("injected upload interruption")
        payload = Path(options["--body"]).read_bytes()
        self.objects[key] = payload, options["--cache-control"]
        self.writes.append(key)
        return {"ETag": hashlib.sha256(payload).hexdigest()}

    def verify(self, key, path, cache_control):
        if self.objects.get(key) != (path.read_bytes(), cache_control):
            raise ValueError(f"Public bytes or cache control differ for {key}")


def snapshot(directory, minor):
    release = {"id": minor, "tag_name": f"v1.{minor}.0", "published_at": "2026-01-01T00:00:00Z"}
    package = f"rpm/Packages/ottrin-1.{minor}.0-1.x86_64.rpm"
    index = directory / f"rpm/repodata/hash-{minor}-primary.xml.gz"
    paths = (
        package, index.relative_to(directory).as_posix(),
        f"apt/pool/main/o/ottrin/ottrin_1.{minor}.0_amd64.deb",
        f"apt/dists/stable/main/binary-amd64/by-hash/SHA256/hash-{minor}",
        *publisher.KEYS, *publisher.POINTERS,
    )
    for key in paths:
        path = directory / key
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(key if key in publisher.KEYS else f"{key} ({minor})")
    (directory / "release.json").write_text(json.dumps(release))
    return release, directory / package, index


def attempt(directory, bucket, work, release):
    latest = {**release, "draft": False, "prerelease": False}
    with patch.object(publisher.subprocess, "check_output", return_value=json.dumps(latest)):
        publisher.publish(directory, bucket, "fixture/Ottrin", work)


with TemporaryDirectory(prefix="ottrin-repository-publication-") as temporary:
    work = Path(temporary)
    bucket = TestBucket()
    old, _, _ = snapshot(work / "old", 1)
    new, package, index = snapshot(work / "new", 2)
    attempt(work / "old", bucket, work, old)
    previous_apt = bucket.objects[publisher.POINTERS[-1]]

    bucket.interrupt = publisher.POINTERS[1]
    try:
        attempt(work / "new", bucket, work, new)
    except OSError as error:
        assert "injected upload interruption" in str(error)
    else:
        raise AssertionError("The interrupted publication reported success")
    assert bucket.objects[publisher.POINTERS[-1]] == previous_apt
    assert json.loads(bucket.objects[publisher.RECEIPT][0])["state"] == "publishing"

    try:
        attempt(work / "old", bucket, work, old)
    except ValueError as error:
        assert "newer or different stable release" in str(error)
    else:
        raise AssertionError("An older release could take over after the interruption")

    bucket.interrupt = None
    attempt(work / "new", bucket, work, new)
    assert bucket.writes[-7:] == [*publisher.POINTERS, publisher.RECEIPT]
    assert json.loads(bucket.objects[publisher.RECEIPT][0])["state"] == "published"
    writes = list(bucket.writes)
    index.rename(index.with_name("rebuilt-primary.xml.gz"))
    attempt(work / "new", bucket, work, new)
    assert bucket.writes == writes

    package.write_text("changed package")
    try:
        attempt(work / "new", bucket, work, new)
    except ValueError as error:
        assert str(error).endswith(package.relative_to(work / "new").as_posix())
    else:
        raise AssertionError("A changed versioned package passed verification")
    assert bucket.writes == writes

print("Repository publication fault, recovery and immutable replay checks passed.")
