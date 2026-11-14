"""Run the Windows package checks serially under one storage lease."""
import json
from pathlib import Path
import signal
import subprocess
import sys

evidence = Path(__file__).resolve().parent
child = None
exits = {}


def stop(signum, _frame):
    raise SystemExit(128 + signum)


def run(name, *args):
    global child
    with (evidence / (name + '.log')).open('w') as log:
        child = subprocess.Popen(
            [sys.executable, str(evidence / args[0]), *args[1:]],
            stdout=log, stderr=subprocess.STDOUT,
        )
        exits[name] = child.wait()
        child = None
    return exits[name] == 0


for sig in (signal.SIGINT, signal.SIGTERM):
    signal.signal(sig, stop)

try:
    run('native-run', 'run-windows.py')
    if run('msix-assembly', 'run-msix-assembly.py') and run('msix-assembly-assessment', 'assess-assembly.py'):
        if run('msix-runtime', 'run-msix-runtime.py'):
            run('msix-runtime-assessment', 'assess-msix-runtime.py', 'runtime40-upgrade')
finally:
    if child is not None and child.poll() is None:
        child.terminate()
        child.wait(timeout=175)
    (evidence / 'NATIVE_COHORT_RESULT.json').write_text(json.dumps(exits, indent=2) + '\n')

# The caller restores any temporarily borrowed compiler outputs before assessments.
assert len(exits) == 5 and all(code == 0 for code in exits.values()), exits
