"""Build one owned four-format Linux cohort; preserve stable deliverables until acceptance."""
from pathlib import Path
import hashlib, importlib.util, json, os, shutil, signal, subprocess, tempfile
root = Path.cwd()
e = Path(__file__).resolve().parent
build = json.loads((e / 'BUILD_SOURCE.json').read_text())
stage = Path(build['stage'])
assert stage == root / '.hermes/packaging-tmp/.linux-picker-stage'
assert subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip() == build['source_revision']
assert not subprocess.check_output(['git', 'status', '--porcelain', '--untracked-files=no'], text=True).strip()
assert subprocess.check_output(['virsh', '-c', 'qemu:///system', 'domstate', 'ottrin-copy-cleanup-qa'], text=True).strip() == 'shut off', 'Avoid compiler/guest memory pressure'
assert not stage.exists()
spec = importlib.util.spec_from_file_location('test_workspace', root / 'scripts/test-workspace.py')
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
    with tempfile.TemporaryDirectory(prefix='.rpm-tools-', dir=e) as directory:
        tool = root / '.hermes/tools/rpm'
        wrapper = Path(directory) / 'rpmbuild'
        wrapper.write_text('#!/bin/sh\nexport LD_LIBRARY_PATH="' + str(tool / 'usr/lib/x86_64-linux-gnu') + '${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"\nexport RPM_CONFIGDIR="' + str(tool / 'usr/lib/rpm') + '"\nexec "' + str(tool / 'usr/bin/rpmbuild') + '" "$@"\n')
        wrapper.chmod(0o700)
        env = {**os.environ, 'CARGO_BUILD_JOBS': '1', 'OTTRIN_DIST_DIR': str(stage),
               'PKG_CONFIG_PATH': str(root / '.hermes/tools/gstreamer-dev/usr/lib/x86_64-linux-gnu/pkgconfig'),
               'PATH': directory + ':' + os.environ['PATH']}
        with (e / 'build.log').open('w') as log:
            child = subprocess.Popen(['bash', 'build-all.sh', '--linux-only'], env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            try:
                code = child.wait()
                assert code == 0, code
            finally:
                workspace.stop_tests(child)
        report['artifacts'] = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in stage.iterdir() if p.is_file() and p.name.endswith(('.AppImage', '.tar.gz', '.deb', '.rpm'))}
        assert len(report['artifacts']) == 4
        report['status'] = 'four-format build pass; staged for content and GUI acceptance'
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
    report['temporary_tool_wrappers_removed'] = not any(e.glob('.rpm-tools-*'))
    report['staging_retained_for_pending_acceptance'] = stage.exists()
    (e / 'BUILD_RESULT.json').write_text(json.dumps(report, indent=2) + '\n')
