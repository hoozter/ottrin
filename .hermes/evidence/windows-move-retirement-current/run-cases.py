"""Owned native cases; observation timeouts resume the same QGA observer."""
from pathlib import Path
import os, shutil, subprocess
ROOT = Path(__file__).resolve().parent
TOOLS = ROOT.parent / 'windows-copy-cleanup-current'
def case(name, arguments, *, reuse=True, ui=False, reuse_receipt=None):
    output = ROOT / name
    output.mkdir()
    build = ROOT / 'ui-build' if ui else ROOT
    for filename in ('build.jsonl', 'build.log'):
        shutil.copy2(build / filename, output / filename)
    command = ['python3', str(TOOLS / 'run-native.py'), *arguments,
               '--evidence-dir', str(output)]
    if reuse:
        command.append('--reuse-binary')
    if reuse_receipt:
        command += ['--reuse-receipt', str(reuse_receipt)]
    if ui:
        command += ['--crate', 'ottrin_ui']
    with (output / 'dispatch.log').open('w') as log:
        subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, check=True)
    environment = os.environ.copy()
    environment['OTTRIN_NATIVE_EVIDENCE_DIR'] = str(output)
    command = ['python3', str(TOOLS / 'observe.py')]
    with (output / 'observe.log').open('w') as log:
        while True:
            result = subprocess.run(command, env=environment, stdout=log, stderr=subprocess.STDOUT)
            if result.returncode != 75:
                result.check_returncode()
                break
            command = ['python3', str(TOOLS / 'observe.py'), 'resume']
    print(name, 'passed', flush=True)
