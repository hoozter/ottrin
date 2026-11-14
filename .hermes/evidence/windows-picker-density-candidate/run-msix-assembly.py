import json
from pathlib import Path
import signal
import subprocess
import sys

evidence = Path(__file__).resolve().parent
driver = evidence / 'windows-assembly-driver.py'
stage = json.loads((evidence / 'HOST_PAYLOAD.json').read_text())['stage']
archive = evidence / 'source.zip'
assert not archive.exists()
child = None

def stop(signum, _frame):
    raise SystemExit(128 + signum)

for sig in (signal.SIGINT, signal.SIGTERM):
    signal.signal(sig, stop)
try:
    for command in (
        [sys.executable, str(evidence / 'prepare-source.py')],
        [sys.executable, str(driver), '--msix-stage', stage, '--output', str(evidence / 'assembly39')],
    ):
        child = subprocess.Popen(command)
        if child.wait():
            raise SystemExit(child.returncode)
finally:
    if child is not None and child.poll() is None:
        child.terminate()
        child.wait(timeout=150)
    archive.unlink(missing_ok=True)
