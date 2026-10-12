#!/usr/bin/env python3
"""Run Cargo tests with disposable app state and the existing Cargo build cache."""

import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile


def cancelled(signum, _frame):
    raise SystemExit(128 + signum)


def stop_tests(process):
    if os.name == "posix":
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
    elif process.poll() is None:
        subprocess.run(
            ["taskkill", "/PID", str(process.pid), "/T", "/F"], check=False
        )
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()
    if os.name == "posix":
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass


def stop_render_jobs(profile):
    # Isolated renderers live in user-systemd units outside Cargo's process group.
    # Their private diagnostic files survive abrupt cancellation of the test.
    if sys.platform != "linux" or not any(profile.rglob(".ottrin-stdout")):
        return
    units = subprocess.run(
        ["systemctl", "--user", "list-units", "--all", "--plain", "--no-legend",
         "--no-pager", "ottrin-render-*.service"],
        check=True, capture_output=True, text=True,
    )
    for line in units.stdout.splitlines():
        unit = line.split()[0]
        command = subprocess.run(
            ["systemctl", "--user", "show", unit, "--property=ExecStart", "--value"],
            check=True, capture_output=True, text=True,
        )
        if str(profile) in command.stdout:
            subprocess.run(["systemctl", "--user", "stop", unit], check=True)


def main():
    for sig in (signal.SIGINT, signal.SIGTERM):
        signal.signal(sig, cancelled)
    operator_home = Path.home()
    env = os.environ.copy()
    env.setdefault("CARGO_HOME", str(operator_home / ".cargo"))
    env.setdefault("RUSTUP_HOME", str(operator_home / ".rustup"))
    with tempfile.TemporaryDirectory(prefix="ottrin-cargo-test-") as temporary:
        profile = Path(temporary)
        for variable, directory in (
            ("HOME", "home"),
            ("USERPROFILE", "home"),
            ("XDG_STATE_HOME", "state"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_DATA_HOME", "data"),
            ("APPDATA", "roaming"),
            ("LOCALAPPDATA", "local"),
            ("TMPDIR", "temporary"),
            ("TMP", "temporary"),
            ("TEMP", "temporary"),
        ):
            path = profile / directory
            path.mkdir(mode=0o700, exist_ok=True)
            env[variable] = str(path)
        for variable in ("DISPLAY", "WAYLAND_DISPLAY"):
            env.pop(variable, None)
        process = subprocess.Popen(
            ["cargo", "test", *(sys.argv[1:] or ["--workspace"])],
            cwd=Path(__file__).resolve().parent.parent,
            env=env,
            start_new_session=os.name == "posix",
        )
        try:
            return process.wait()
        finally:
            stop_tests(process)
            stop_render_jobs(profile)


if __name__ == "__main__":
    sys.exit(main())
