#!/usr/bin/python3
"""Run the search interaction smoke checks on an owned, private X11 desktop."""
import argparse
import hashlib
from pathlib import Path
import sys

from session import OwnedDesktop


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    evidence = args.evidence.resolve()
    root = Path(__file__).resolve().parents[2]
    with OwnedDesktop(evidence) as desktop:
        desktop.env.pop("AT_SPI_BUS_ADDRESS", None)
        desktop.env["NO_AT_BRIDGE"] = "0"
        desktop.report["binary"] = str(binary)
        desktop.report["sha256"] = hashlib.sha256(binary.read_bytes()).hexdigest()
        process = desktop.spawn(
            ["/usr/bin/python3", str(root / "scripts/linux-desktop-qa.py"),
             "--binary", str(binary), "--evidence", str(evidence),
             "--search-interactions-only"],
            "checks.log",
        )
        result = process.wait(timeout=180)
        print((evidence / "checks.log").read_text())
        if result:
            raise RuntimeError(f"Search interaction QA exited {result}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
