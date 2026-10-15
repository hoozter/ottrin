#!/usr/bin/env python3
"""Real Linux daemon ownership across unprivileged private mount namespaces.

Requires an ordinary user, enabled user/mount namespaces and a release indexd.
Profiles, processes and mount sources are disposable and always cleaned up.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--in-user-namespace", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    if os.getuid() == 0:
        parser.error("Run as an ordinary user; the fixture maps that UID unchanged")
    if not args.in_user_namespace:
        # Share one user namespace so ptrace/proc ownership checks are not
        # denied merely because the two test processes are in sibling user
        # namespaces. Only their filesystem mount views differ.
        os.execvp("unshare", ["unshare", "--user", "--map-current-user", "--keep-caps",
                             "/usr/bin/python3", str(Path(__file__).resolve()),
                             "--in-user-namespace", *sys.argv[1:]])
    def cancelled(_signum, _frame):
        raise KeyboardInterrupt("Namespace fixture cancelled")

    signal.signal(signal.SIGTERM, cancelled)
    wrapper = (
        "import os,ctypes,sys;"
        "libc=ctypes.CDLL('libc.so.6',use_errno=True);"
        "r=libc.mount(os.fsencode(sys.argv[1]),b'/usr/bin',None,4096,None);"
        "assert r==0,os.strerror(ctypes.get_errno());"
        "os.execv('/usr/bin/ottrin-indexd',['/usr/bin/ottrin-indexd']+sys.argv[2:])"
    )
    launcher = ["unshare", "--mount",
                "--propagation", "private", "--keep-caps", "/usr/bin/python3", "-c", wrapper]
    result = {"binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(), "cases": []}
    for case in ["native-private-namespace", "foreign-private-namespace", "native-deleted-executable"]:
        row = {"case": case, "status": "running"}
        try:
            # Keep the Unix socket path within sockaddr_un's length limit.
            with tempfile.TemporaryDirectory(prefix="ottrin-owner-") as directory:
                root = Path(directory)
                native = root / "native-bin"
                foreign = root / "foreign-bin"
                for name in ["native-bin", "foreign-bin", "home", "config/ottrin", "cache", "runtime", "fixtures"]:
                    (root / name).mkdir(parents=True, exist_ok=True)
                (root / "runtime").chmod(0o700)
                source = root / "fixtures/source.txt"
                source.write_text("Preserved original\n")
                config = root / "config/ottrin/config.json"
                config.write_text(json.dumps({"search_setup_completed": True, "search": {
                    "include_roots": [str(root / "fixtures")], "content_indexing_enabled": False}}))
                original_config = config.read_bytes()
                for target in [native / "ottrin-indexd", foreign / "ottrin-indexd"]:
                    shutil.copy2(binary, target)
                env = os.environ.copy()
                env.update(HOME=str(root / "home"), XDG_CONFIG_HOME=str(root / "config"),
                           XDG_CACHE_HOME=str(root / "cache"), XDG_RUNTIME_DIR=str(root / "runtime"))
                peer_bin = foreign if case == "foreign-private-namespace" else native
                with tempfile.TemporaryFile(mode="w+t") as log:
                    child = subprocess.Popen(launcher + [str(peer_bin), "--index-daemon"], env=env,
                                             stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
                    try:
                        socket = root / "runtime/ottrin/index-v1.sock"
                        deadline = time.monotonic() + 12
                        while not socket.exists() and child.poll() is None and time.monotonic() < deadline:
                            time.sleep(0.05)
                        if not socket.exists():
                            log.seek(0)
                            raise RuntimeError("Private daemon did not start: " + log.read())
                        row["peer_executable"] = os.readlink(f"/proc/{child.pid}/exe")
                        assert row["peer_executable"] == "/usr/bin/ottrin-indexd"
                        if case == "native-deleted-executable":
                            (native / "ottrin-indexd").unlink()
                            shutil.copy2(binary, native / "ottrin-indexd")
                            row["peer_executable"] = os.readlink(f"/proc/{child.pid}/exe")
                            assert row["peer_executable"] == "/usr/bin/ottrin-indexd (deleted)"
                        own = (native / "ottrin-indexd").stat()
                        peer = Path(f"/proc/{child.pid}/root/usr/bin/ottrin-indexd").stat()
                        row["same_installed_entry"] = (own.st_dev, own.st_ino) == (peer.st_dev, peer.st_ino)
                        assert row["same_installed_entry"] == (case != "foreign-private-namespace")
                        cleanup = subprocess.run(launcher + [str(native), "--native-package-stop"], env=env,
                                                 capture_output=True, text=True, timeout=40)
                        row["cleanup_exit"] = cleanup.returncode
                        assert cleanup.returncode == 0, cleanup.stderr
                        if case == "foreign-private-namespace":
                            assert child.poll() is None, "Cleanup stopped an independent namespace daemon"
                        else:
                            assert child.wait(timeout=3) == 0
                        assert config.read_bytes() == original_config
                        assert source.read_text() == "Preserved original\n"
                        row["status"] = "pass"
                    finally:
                        # Reap before the private runtime, DB and mount source disappear.
                        if child.poll() is None:
                            os.killpg(child.pid, signal.SIGTERM)
                        try:
                            child.wait(timeout=8)
                        except subprocess.TimeoutExpired:
                            os.killpg(child.pid, signal.SIGKILL)
                            child.wait(timeout=5)
                        row["process_reaped"] = child.poll() is not None
        except Exception as error:
            row.update(status="fail", error=str(error))
        result["cases"].append(row)
    result["status"] = "pass" if all(r["status"] == "pass" for r in result["cases"]) else "fail"
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))
    return 0 if result["status"] == "pass" else 1


if __name__ == "__main__":
    raise SystemExit(main())
