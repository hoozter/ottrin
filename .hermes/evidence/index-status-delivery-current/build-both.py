"""Build serially within shared caches; never change HEAD during provenance capture."""
from pathlib import Path
import importlib.util
import json
import signal
import subprocess
import sys

sys.dont_write_bytecode = True
root = Path.cwd()
evidence = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('workspace', root / 'scripts/test-workspace.py')
workspace = importlib.util.module_from_spec(spec)
spec.loader.exec_module(workspace)
for sig in (signal.SIGINT, signal.SIGTERM):
    signal.signal(sig, workspace.cancelled)
state = {'status': 'starting', 'source_revision': json.loads((evidence / 'BUILD_SOURCE.json').read_text())['source_revision'],
         'rule': 'No tracked edits or commits until both builds terminate; each packager reads real Git provenance.',
         'completed': [], 'stable_deliverables': 'Linux/Windows b020 stable deliverables preserved', 'goal': 'active/incomplete'}
child = None


def save():
    (evidence / 'ACTIVE_HANDLES.json').write_text(json.dumps(state, indent=2) + '\n')


try:
    for name, script in [('Linux', evidence / 'build.py'), ('Windows', evidence / 'windows/build-portable.py')]:
        state.update(status='building', phase=name)
        child = subprocess.Popen([sys.executable, str(script)], start_new_session=True)
        state['producer_pid'] = child.pid
        save()
        print(f'{name} producer started: {child.pid}', flush=True)
        try:
            code = child.wait()
            assert code == 0, (name, code)
        finally:
            workspace.stop_tests(child)
        state['completed'].append(name)
        print(f'{name} build terminal pass', flush=True)
    state['status'] = 'both builds terminal; staged acceptance required'
except BaseException as error:
    state.update(status='build interrupted or failed; inspect each producer receipt', error=repr(error))
    raise
finally:
    if child:
        workspace.stop_tests(child)
    state['producer_exit'] = child.poll() if child else None
    save()
