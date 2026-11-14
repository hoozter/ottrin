"""Borrow space from generated test executables while the installed fixture runs."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time

evidence = Path(__file__).resolve().parent
repo = evidence.parents[2]
deps = repo / 'target/x86_64-pc-windows-gnu/debug/deps'
save_identity = os.environ.get('OTTRIN_QA_SAVE_IDENTITY') == '1'
stable_save = os.environ.get('OTTRIN_QA_SAVE_IDENTITY_STABLE') == '1'
assert not stable_save or save_identity
results = evidence
arguments = sys.argv[1:]
assert arguments in ([], ['assembly'], ['runtime'])
fixture_name, space_receipt = {
    (): ('run-windows.py', 'WINDOWS_PICKER_SPACE.json'),
    ('assembly',): ('run-msix-assembly.py', 'MSIX_ASSEMBLY_SPACE.json'),
    ('runtime',): ('run-msix-runtime.py', 'MSIX_RUNTIME_SPACE.json'),
}[tuple(arguments)]
fixture = evidence / fixture_name
names = (
    'ottrin_platform-e2d6933b1ac22a07.exe',
    'ottrin_ui-5d9606400e36b794.exe',
    'ottrin_app-b14e5f4b4a3e18bf.exe',
    'ottrin_indexd-2d6761405629f9c1.exe',
    'ottrin_platform-d1588445e089e3c2.exe',
    'native_office_boundary_contract-b59b07b52984880e.exe',
    'ottrin_preview-207cb97347cf7fbd.exe',
    'office_provider_contract-3f00708fd370a3bb.exe',
    'windows_job_native-e79be56cfa052a12.exe',
    'eps_visual-dac11049dee4afa2.exe',
    'windows_office_boundary_contract-c7f004f2b3501443.exe',
    'windows_job_native-1a516e3a54ed94e9.exe',
    'ottrin_search-c17f9693eb7d1e62.exe',
    'windows_daemon-905dc7cbc4e65da4.exe',
    'windows_office_boundary_contract-c2a4718f3d03b9cf.exe',
    'windows_job_native-d126374b8b123e5b.exe',
)
paths = [deps / name for name in names]
paths.extend(repo / 'target/debug/deps' / name for name in (
    'ottrin_ui-c354643af000a7cc',
    'ottrin_app-f0e299e85f6fae93',
    'ottrin_indexd-987f16c652a12bbc',
    'ottrin_preview-89d8d7d374b9faf4',
    'native_office_boundary_contract-b917bd910e5af84c',
    'pdf_visual-3bcb035c8242a571',
    'eps_visual-3eaab3c067d4c766',
    'office_provider_contract-4346291d9de4515b',
    'html_visual-ab1ee9e189d5119d',
    'media_playback_contract-9e1d4bb90a7577ce',
    'video_thumbnail_contract-6ac8b6de028e3979',
    'ottrin_ui-b21e7538cbb86b0a',
    'ottrin_platform-274f7976d82372f5',
    'ottrin_search-ea268e832702de9b',
    'ottrin_platform-5a95d0376980cc28',
    'ottrin_priv_helper-b5a1cf66b9476b20',
    'persisted_search_cli-e70a1f794d02c475',
    'persisted_search_cli-05e92570c7bd4c05',
    'ottrin_priv_helper-4c94f065786697f2',
    'ottrin_priv_helper-54132b059e6f738d',
))
paths_set = set(paths)
vm_path = '/var/lib/libvirt/images'
free = lambda: shutil.disk_usage(vm_path).free

def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()

def decoded_digest(archive):
    with subprocess.Popen(['zstd', '-dc', str(archive)], stdout=subprocess.PIPE) as proc:
        value = hashlib.file_digest(proc.stdout, 'sha256').hexdigest()
        assert proc.wait() == 0
        return value

def stop(_signal, _frame):
    raise KeyboardInterrupt('Installed fixture interrupted')

for sig in (signal.SIGINT, signal.SIGTERM):
    signal.signal(sig, stop)

proc_names = subprocess.check_output(['ps', '-eo', 'comm='], text=True).splitlines()
assert not {'cargo', 'rustc', 'rust-analyzer'}.intersection(proc_names)
for path in paths:
    assert path.is_file() and not path.is_symlink() and path.stat().st_nlink == 1
    with path.open('rb') as stream:
        assert stream.read(4).startswith(b'MZ' if path.suffix == '.exe' else b'\x7fELF')
    markers = list((path.parent.parent / '.fingerprint').glob('*-' + path.stem.rsplit('-', 1)[1] + '/test-*.json'))
    assert len(markers) == 1 and markers[0].with_suffix('').is_file()
    assert subprocess.run(['fuser', str(path)], capture_output=True).returncode == 1

roots = [repo, Path.home()/'.local/bin', Path.home()/'.local/share/applications', Path.home()/'.config/systemd/user', Path.home()/'.config/autostart']
links = subprocess.check_output(['find', *[str(p) for p in roots if p.exists()], '-xdev', '-type', 'l', '-print0'])
for item in links.split(b'\0'):
    if item:
        assert Path(os.fsdecode(item)).resolve() not in paths_set

before = free()
rows = []
child = None
status = None
started = None
lowest = None
stage = Path(tempfile.mkdtemp(prefix='.alias-compressed-tests-', dir=repo / '.hermes/packaging-tmp'))
archive_stage = None
print(f'Restore manifest directory {stage}', flush=True)
try:
    memory = Path('/dev/shm')
    assert subprocess.check_output(['stat', '-f', '-c', '%T', str(memory)], text=True).strip() == 'tmpfs'
    assert memory.stat().st_dev != stage.stat().st_dev
    assert shutil.disk_usage(memory).free >= 4 * 1024**3
    archive_stage = Path(tempfile.mkdtemp(prefix='ottrin-picker-resource-archives-', dir=memory))
    print(f'Reproducible test executable archives {archive_stage}', flush=True)
    for path in paths:
        if free() >= 32 * 1024**3:
            break
        assert shutil.disk_usage(archive_stage).free >= 768 * 1024**2
        info = path.stat()
        archive = archive_stage / (path.name + '.zst')
        original = digest(path)
        subprocess.run(['zstd', '-q', '-3', '-o', str(archive), str(path)], check=True)
        assert decoded_digest(archive) == original
        assert path.stat().st_mtime_ns == info.st_mtime_ns and digest(path) == original
        row = {'path': str(path), 'archive': str(archive), 'sha256': original, 'size': info.st_size,
               'mode': info.st_mode & 0o777, 'atime_ns': info.st_atime_ns, 'mtime_ns': info.st_mtime_ns}
        rows.append(row)
        (stage/'RESTORE.json').write_text(json.dumps(rows, indent=2)+'\n')
        path.unlink()
    started = lowest = free()
    # The previous stock-client run consumed less than 11 GiB at peak.
    # Keep 2 GiB more than that plus the existing 16 GiB emergency floor.
    # The driver independently enforces its 24 GiB admission floor.
    assert started >= 29 * 1024**3, started
    print(f'Starting installed Windows fixture with {started} bytes free', flush=True)
    child = subprocess.Popen([sys.executable, str(fixture)] + [], start_new_session=True)
    while child.poll() is None:
        lowest = min(lowest, free())
        if lowest < 16 * 1024**3:
            raise RuntimeError('Installed fixture reached the emergency storage floor')
        time.sleep(2)
    status = child.returncode
finally:
    for sig in (signal.SIGINT, signal.SIGTERM):
        signal.signal(sig, signal.SIG_IGN)
    if child is not None and child.poll() is None:
        child.terminate()
        child.wait(timeout=180)
    for row in reversed(rows):
        path = Path(row['path'])
        if not path.exists():
            with tempfile.NamedTemporaryFile(dir=deps, prefix='.restore-alias-test-', delete=False) as out:
                restored = Path(out.name)
                try:
                    subprocess.run(['zstd', '-dc', row['archive']], stdout=out, check=True)
                    out.flush()
                    assert digest(restored) == row['sha256']
                    os.chmod(restored, row['mode'])
                    os.utime(restored, ns=(row['atime_ns'], row['mtime_ns']))
                    os.link(restored, path)
                finally:
                    restored.unlink()
        assert digest(path) == row['sha256']
    if archive_stage is not None:
        shutil.rmtree(archive_stage)
    shutil.rmtree(stage)
    result = {'fixture_exit': status, 'free_before': before, 'free_at_fixture_start': started,
              'lowest_observed_free': lowest, 'free_after': free(),
              'restored_by_hash': [{k: row[k] for k in ('path', 'sha256')} for row in rows],
              'compression_stage_removed': not stage.exists() and (archive_stage is None or not archive_stage.exists())}
    (results / space_receipt).write_text(json.dumps(result, indent=2)+'\n')
    print(json.dumps(result), flush=True)
raise SystemExit(status)
