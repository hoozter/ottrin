#!/usr/bin/env python3
"""Exercise an explicitly built Linux helper; does not certify GUI/audio acceptance.

Requires ffmpeg to generate a tiny silent video. Uses installed GStreamer only.
All fixtures and plugin registries live in an automatically removed temp directory.
"""
import argparse
import fcntl
import hashlib
import os
from pathlib import Path
import socket
import struct
import subprocess
import tempfile
import time


def receive(control):
    def exact(size):
        chunks = bytearray()
        while len(chunks) < size:
            chunk = control.recv(size - len(chunks))
            if not chunk:
                raise AssertionError("helper closed its event stream unexpectedly")
            chunks.extend(chunk)
        return bytes(chunks)

    kind = exact(1)[0]
    if kind == 4:
        return kind, exact(1)[0]
    if kind == 5:
        size = struct.unpack("<I", exact(4))[0]
        assert size <= 65536, "unbounded error"
        return kind, exact(size).decode("utf-8")
    if kind == 6:
        width, height, size = struct.unpack("<III", exact(12))
        assert 0 < width <= 4096 and 0 < height <= 4096
        assert size == width * height * 4
        return kind, (width, height, exact(size))
    assert kind in (1, 2, 3), f"unknown event {kind}"
    return kind, None


def probe(helper, fixture, directory, mode):
    source = os.open(fixture, os.O_RDONLY | os.O_NOFOLLOW)
    source_copy = fcntl.fcntl(source, fcntl.F_DUPFD_CLOEXEC, 10)
    os.close(source)
    parent, child = socket.socketpair()
    control_copy = fcntl.fcntl(child.fileno(), fcntl.F_DUPFD_CLOEXEC, 10)

    def inherit():
        assert control_copy is not None
        os.dup2(source_copy, 3, inheritable=True)
        os.dup2(control_copy, 4, inheritable=True)

    env = os.environ.copy()
    env["GST_REGISTRY"] = str(directory / f"registry-{mode}.bin")
    if mode == "missing-components":
        env["GST_PLUGIN_PATH"] = env["GST_PLUGIN_PATH_1_0"] = ""
        env["GST_PLUGIN_SYSTEM_PATH"] = env["GST_PLUGIN_SYSTEM_PATH_1_0"] = ""
    try:
        # Replace the pathname after admission; playback must use the retained FD.
        if mode == "video":
            replacement = directory / "replacement"
            replacement.write_bytes(b"not the admitted video")
            replacement.replace(fixture)
        with tempfile.TemporaryFile() as errors:
            process = subprocess.Popen(
                [str(helper), "--inherited-media-session"],
                preexec_fn=inherit, close_fds=False,
                stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                stderr=errors, env=env,
            )
            try:
                child.close()
                os.close(control_copy)
                control_copy = None
                deadline = time.monotonic() + 15
                frames = 0
                paused = False
                while True:
                    remaining = deadline - time.monotonic()
                    assert remaining > 0, "playback deadline exceeded"
                    parent.settimeout(remaining)
                    kind, value = receive(parent)
                    if mode == "missing-components" and kind == 4:
                        assert value == 2, f"unexpected fallback {value}"
                        break
                    if mode == "malformed" and kind == 5:
                        assert value, "missing diagnostic"
                        break
                    assert kind not in (4, 5), (kind, value)
                    if kind == 6:
                        assert isinstance(value, tuple)
                        width, height, rgba = value
                        assert (width, height) == (32, 24)
                        assert rgba[0] > 200 and rgba[1] < 40 and rgba[2] < 40
                        frames += 1
                        if mode == "stop":
                            parent.sendall(b"\x03")
                            break
                        if frames == 1:
                            parent.sendall(b"\x02")
                    elif kind == 2:
                        paused = True
                        parent.sendall(b"\x01")
                    elif kind == 3:
                        assert mode == "video" and frames > 1 and paused
                        break
                assert process.wait(timeout=3) == 0
                print(f"PASS {mode}: frames={frames}, pause={paused}")
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait()
                errors.seek(0)
                diagnostic = errors.read(16384).decode("utf-8", errors="replace")
                if process.returncode:
                    print(diagnostic)
    finally:
        parent.close()
        child.close()
        os.close(source_copy)
        if control_copy is not None:
            os.close(control_copy)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("helper", type=Path)
    args = parser.parse_args()
    helper = args.helper.resolve(strict=True)
    with helper.open("rb") as binary:
        digest = hashlib.file_digest(binary, "sha256").hexdigest()
    print(f"helper={helper}\nsha256={digest}", flush=True)
    with tempfile.TemporaryDirectory(prefix="ottrin-media-smoke-") as temporary:
        directory = Path(temporary)
        video = directory / "video.avi"
        subprocess.run([
            "ffmpeg", "-v", "error", "-f", "lavfi", "-i",
            "color=c=red:s=32x24:r=10:d=2", "-an", "-c:v", "rawvideo",
            "-pix_fmt", "yuv420p", str(video),
        ], check=True, timeout=15)
        malformed = directory / "malformed.avi"
        malformed.write_bytes(b"this is not media")
        probe(helper, malformed, directory, "missing-components")
        probe(helper, malformed, directory, "malformed")
        probe(helper, video, directory, "stop")
        probe(helper, video, directory, "video")
    print("PASS temporary fixtures and plugin registries removed")


if __name__ == "__main__":
    main()
