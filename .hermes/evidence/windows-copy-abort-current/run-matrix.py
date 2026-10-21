"""Run the current-source staging-end and native Activity checks serially."""
from pathlib import Path
import json, os, shutil, subprocess, sys
p = Path(__file__).resolve().parent
runner = p.parent / 'windows-copy-cleanup-current'
core = json.loads((p / 'core/RESULTS.json').read_text(encoding='utf-8-sig'))
assert core['status'] == 'pass' and core['exit'] == 0
for name, selection in [
    ('abort-final', ['--copy-abort']),
    ('ui-state', ['--crate', 'ottrin_ui', '--exact', 'tests::windows_retained_copy_activity_keeps_evidence_and_actions_separate']),
    ('activity', ['--crate', 'ottrin_ui', '--exact', 'tests::windows_copy_recovery_activity_integration']),
]:
    case = p / name
    case.mkdir()
    shutil.copy2(p / 'build.jsonl', case / 'build.jsonl')
    with (case / 'dispatch.log').open('w') as log:
        subprocess.run([sys.executable, str(runner / 'run-native.py'), '--evidence-dir', str(case), *selection], stdout=log, stderr=subprocess.STDOUT, check=True)
    env = dict(os.environ, OTTRIN_NATIVE_EVIDENCE_DIR=str(case))
    result = subprocess.run([sys.executable, str(runner / 'observe.py')], env=env)
    while result.returncode == 75:
        result = subprocess.run([sys.executable, str(runner / 'observe.py'), 'resume'], env=env)
    result.check_returncode()
    print(name + ': pass', flush=True)
