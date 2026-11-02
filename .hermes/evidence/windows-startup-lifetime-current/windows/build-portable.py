"""Stage the current portable/NSIS payload; preserve the usable Windows candidate."""
from pathlib import Path
import argparse, hashlib, importlib.util, json, os, shutil, signal, subprocess
r = Path.cwd()
parser = argparse.ArgumentParser()
parser.add_argument('--output', type=Path, required=True)
args = parser.parse_args()
e = args.output.resolve()
assert e.is_dir() and not (e / 'BUILD_RESULT.json').exists(), 'Existing build receipt must not be overwritten'
build = json.loads((e / 'BUILD_SOURCE.json').read_text())
stage = Path(build['stage'])
assert stage == r / '.hermes/packaging-tmp/.windows-desktop-stage'
assert subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip() == build['source_revision']
assert not subprocess.check_output(['git', 'status', '--porcelain', '--untracked-files=no'], text=True).strip()
assert subprocess.check_output(['virsh', '-c', 'qemu:///system', 'domstate', 'ottrin-copy-cleanup-qa'], text=True).strip() == 'shut off'
assert not stage.exists()
spec = importlib.util.spec_from_file_location('test_workspace', r / 'scripts/test-workspace.py')
workspace = importlib.util.module_from_spec(spec)
spec.loader.exec_module(workspace)
for sig in [signal.SIGTERM, signal.SIGINT]:
    signal.signal(sig, workspace.cancelled)
child = None
owned = False
report = {'source_revision': build['source_revision'], 'stage': str(stage), 'status': 'starting'}
try:
    stage.mkdir()
    owned = True
    env = {**os.environ, 'CARGO_BUILD_JOBS': '1', 'OTTRIN_DIST_DIR': str(stage)}
    for name, command in [
        ('portable-build.log', ['bash', 'packaging/windows/package-portable.sh']),
        ('nsis-build.log', ['bash', 'packaging/windows/package-installer.sh']),
    ]:
        with (e / name).open('w') as log:
            child = subprocess.Popen(command, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            try:
                code = child.wait()
                assert code == 0, (name, code)
            finally:
                workspace.stop_tests(child)
    report['artifacts'] = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in stage.iterdir() if p.is_file() and p.name.endswith(('.zip', '.exe'))}
    assert len(report['artifacts']) == 2
    report['status'] = 'portable and NSIS build pass; staged for native MSI and GUI acceptance'
except BaseException as error:
    if child:
        workspace.stop_tests(child)
    report['status'] = 'failed; owned staging retired'
    report['error'] = repr(error)
    if owned:
        files = [p for p in stage.rglob('*') if p.is_file() and not p.is_symlink()]
        assert all(p.stat().st_nlink == 1 for p in files)
        if files:
            assert subprocess.run(['fuser', *map(str, files)], capture_output=True).returncode == 1
        shutil.rmtree(stage)
    raise
finally:
    if child:
        workspace.stop_tests(child)
    report['child_exit'] = child.poll() if child else None
    report['staging_retained_for_pending_acceptance'] = stage.exists()
    (e / 'BUILD_RESULT.json').write_text(json.dumps(report, indent=2) + '\n')
