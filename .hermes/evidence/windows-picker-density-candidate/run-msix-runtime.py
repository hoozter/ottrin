import json
from pathlib import Path
import signal
import subprocess
import sys

evidence = Path(__file__).resolve().parent
driver = evidence / 'windows-assembly-driver.py'
artifact = json.loads((evidence / 'ASSEMBLY_ACCEPTANCE.json').read_text())['artifact']
previous = json.loads((evidence / 'PREVIOUS_PACKAGE.json').read_text())['retained_input']
child = None

def stop(signum, _frame):
    raise SystemExit(128 + signum)

for sig in (signal.SIGINT, signal.SIGTERM):
    signal.signal(sig, stop)
try:
    for case, arguments in (('runtime39-upgrade', ['--msix-upgrade-from', previous]),):
        child = subprocess.Popen([
            sys.executable, str(driver), '--msix-runtime', artifact,
            '--output', str(evidence / case), '--use-stock-base', *arguments,
        ])
        if child.wait():
            raise SystemExit(child.returncode)
        result = json.loads((evidence / case / 'RUNTIME_RESULT.json').read_text())
        assert result['status'] == 'pass', (case, result['status'])
finally:
    if child is not None and child.poll() is None:
        child.terminate()
        child.wait(timeout=150)
