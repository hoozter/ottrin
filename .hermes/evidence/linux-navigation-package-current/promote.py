"""Publish the scoped Linux cohort after source, contents and actual-AppImage checks."""
from pathlib import Path
import hashlib, json, shutil, subprocess

root = Path.cwd()
p = Path(__file__).resolve().parent
public = root / 'dist/packages'

def read(path):
    return json.loads(path.read_text())

def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()

build = read(p / 'BUILD_SOURCE.json')
content = read(p / 'PACKAGE_CONTENT_ACCEPTANCE.json')
hosted = read(p / 'HOSTED_SOURCE.json')
gui = read(p / 'GUI_ACCEPTANCE.json')
native = read(p.parent / 'navigation-toolbar-current/SOURCE_CHECKS.json')
source = build['source_revision']
assert source == content['source_revision'] == hosted['headSha'] == gui['source_revision']
assert source == subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
assert not subprocess.check_output(['git', 'status', '--porcelain', '--untracked-files=no'], text=True).strip()
assert sha(root / 'crates/ottrin-ui/src/lib.rs') == native['source_sha256']
assert native['linux']['regular_ui'] == 410 and native['windows']['ordinary_regular_ui'] == native['windows']['elevated_regular_ui'] == 383
for name in ['verify (ubuntu-latest)', 'search-performance']:
    assert next(job for job in hosted['jobs'] if job['name'] == name)['conclusion'] == 'success', name
assert gui['status'] == 'pass within scoped actual-AppImage navigation and metadata acceptance'
for folder in ['dark-list100', 'light-list150']:
    teardown = read(p / folder / 'TEARDOWN.json')
    assert teardown['normal_app_exit'] and teardown['profile_removed']
    assert teardown['artifact_sha256'] == content['artifacts']['Ottrin-0.1.0-x86_64.AppImage']
    assert all(process['exit'] is not None for process in teardown['owned_processes_reaped'])
    assert gui['sessions'][folder]['source_bytes_match']
stage = Path(build['stage'])
extract = Path(content['extraction'])
for name, digest in read(p / 'DELIVERABLE_BASELINE.json').items():
    assert sha(public / name) == digest, name
for name, digest in content['artifacts'].items():
    assert sha(stage / name) == digest
    assert (stage / (name + '.sha256')).read_text().split() == [digest, name]
previous = read(public / 'LINUX_PACKAGES_ACCEPTANCE.json')
index = sha(extract / 'squashfs-root/usr/bin/ottrin-indexd')
html = sha(extract / 'squashfs-root/usr/libexec/ottrin/ottrin-html-render')
assert index == previous['indexd_binary_sha256'] and html == previous['html_binary_sha256']
files = [f for directory in [stage, extract] for f in directory.rglob('*') if f.is_file() and not f.is_symlink()]
assert all(f.stat().st_nlink == 1 for f in files)
assert subprocess.run(['fuser', *map(str, files)], capture_output=True).returncode == 1
receipt = {
    'status': 'pass within Linux source,four-format contents and scoped actual-AppImage acceptance',
    'source_revision': source, 'artifacts': content['artifacts'], 'evidence': str(p.relative_to(root)),
    'checks': content['checks'] + gui['checks'], 'gui_acceptance': 'GUI_ACCEPTANCE.json',
    'indexd_binary_sha256': index, 'html_binary_sha256': html,
    'indexd_resource_evidence': previous['indexd_resource_evidence'],
    'indexd_resource_scope': 'Byte-identical daemon; earlier resource observations retain their original dates,workloads and artifact scopes. No new CPU or memory acceptance claimed.',
    'native_source_evidence': 'navigation-toolbar-current/SOURCE_CHECKS.json',
    'hosted_source_evidence': {'run': 41251463739, 'source_revision': source, 'linux_job': 'success', 'performance_job': 'success', 'windows_job_at_linux_promotion': next(job for job in hosted['jobs'] if job['name'] == 'verify (windows-latest)')['status']},
    'scope': 'Usable local Linux preview,not a public release or final daily-driver acceptance. Windows,installer rollback,Office providers,physical devices/GPU,signing and owner acceptance remain separate.',
    'cleanup': 'Both private GUI sessions closed normally and removed owned profiles,processes,displays,buses and mounts. Completed staging/extraction retired after publication. Current shared compiler outputs and compact evidence retained.'
}
(p / 'PREVIOUS_LINUX_PACKAGES_ACCEPTANCE.json').write_text(json.dumps(previous, indent=2) + '\n')
for name, digest in content['artifacts'].items():
    subprocess.run(['bash', '-c', 'set -euo pipefail;source "$1";ottrin_publish_artifact "$2" "$3"', 'publish', str(root / 'packaging/package-lifecycle.sh'), str(stage / name), str(public / name)], check=True)
    for suffix in ['.sha256', '.provenance.json']:
        sidecar = stage / (name + suffix)
        if sidecar.exists():
            shutil.move(sidecar, public / sidecar.name)
    assert sha(public / name) == digest
(public / 'LINUX_PACKAGES_ACCEPTANCE.json').write_text(json.dumps(receipt, indent=2) + '\n')
(p / 'ACCEPTANCE.json').write_text(json.dumps(receipt, indent=2) + '\n')
assert not any(stage.iterdir())
stage.rmdir()
shutil.rmtree(extract)
build.update(pending_consumers=[], cleanup_complete=True, delivery_status='Accepted scoped Linux preview,published;staging/extraction retired')
(p / 'BUILD_SOURCE.json').write_text(json.dumps(build, indent=2) + '\n')
print('Published four verified Linux formats; owned staging/extraction retired')
