#!/usr/bin/python3
"""Run packaged-desktop QA on an owned disposable desktop, never the host session.

Wraps ``scripts/linux-desktop-qa.py`` in the maintained ``OwnedDesktop``
lifecycle (private Xvfb, private session bus, KWin, /var/tmp profile with
teardown).  The accessibility bridge is deliberately re-enabled on that
private bus because OwnedDesktop suppresses it by default and the QA checks
read the app through AT-SPI.
"""
from __future__ import annotations

import argparse
import importlib.util
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SESSION = ROOT / "scripts/linux-desktop-qa/session.py"


def load_session():
    spec = importlib.util.spec_from_file_location("linux_desktop_qa_session", SESSION)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def enable_a11y(env) -> None:
    """Enable the accessibility bus on the owned session before the QA preflight."""
    call = [
        "gdbus", "call", "--session", "--dest", "org.a11y.Bus",
        "--object-path", "/org/a11y/bus", "--method",
        "org.freedesktop.DBus.Properties.Set", "org.a11y.Status", "IsEnabled", "<true>",
    ]
    failure = None
    for _ in range(20):
        result = subprocess.run(call, env=env, text=True, capture_output=True, timeout=15)
        if result.returncode == 0:
            return
        failure = result.stderr
        time.sleep(0.25)
    raise RuntimeError(f"could not enable the owned accessibility bus: {failure}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--binary",
        type=Path,
        default=Path("dist/Ottrin-0.1.0-linux-x86_64/bin/ottrin"),
    )
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument(
        "--mode",
        choices=("full", "preview", "search-interactions"),
        default="search-interactions",
    )
    args = parser.parse_args()

    binary = args.binary.resolve()
    if not binary.is_file():
        parser.error(f"packaged binary not found: {binary}")

    session = load_session()
    evidence = args.evidence.resolve()
    evidence.mkdir(parents=True, exist_ok=True)
    with session.OwnedDesktop(evidence) as owned:
        # OwnedDesktop suppresses the accessibility bridge by default; the
        # QA script reads the app through AT-SPI, so re-enable it on the
        # private bus and let the app inherit the owned registry.
        owned.env.update(NO_AT_BRIDGE="0")
        owned.env.pop("AT_SPI_BUS_ADDRESS", None)
        subprocess.run(
            ["gsettings", "set", "org.gnome.desktop.interface", "toolkit-accessibility", "true"],
            env=owned.env,
            check=False,
            capture_output=True,
            timeout=15,
        )
        enable_a11y(owned.env)
        command = [
            sys.executable,
            str(ROOT / "scripts/linux-desktop-qa.py"),
            "--binary",
            str(binary),
            "--evidence",
            str(evidence / "qa"),
        ]
        if args.mode == "preview":
            command.append("--preview-only")
        elif args.mode == "search-interactions":
            command.append("--search-interactions-only")
        return subprocess.run(command, env=owned.env, cwd=ROOT, timeout=1200).returncode


if __name__ == "__main__":
    raise SystemExit(main())
