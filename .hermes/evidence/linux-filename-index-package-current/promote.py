"""Promote only the exact accepted four-format cohort; retire its completed staging."""
from pathlib import Path
import gzip
import hashlib
import json
import shutil
import subprocess

root = Path.cwd()
evidence = root / '.hermes/evidence/linux-filename-index-package-current'
public = root / 'dist/packages'

def read(name):
    return json.loads((evidence / name).read_text())

def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()

build = read('BUILD_SOURCE.json')
content = read('PACKAGE_CONTENT_ACCEPTANCE.json')
stage = Path(build['stage'])
extract = Path(content['extraction'])
hosted = json.loads((root / '.hermes/evidence/index-fts-build-probe-current/HOSTED_LINUX_ACCEPTANCE.json').read_text())
assert content['source_revision'] == hosted['package_source_revision'] == build['source_revision']
assert not subprocess.check_output(['git', 'status', '--porcelain', '--untracked-files=no'], text=True).strip()
assert not subprocess.check_output(['git', 'diff', hosted['source_revision'], build['source_revision'], '--', 'crates', 'Cargo.toml', 'Cargo.lock', 'packaging', 'scripts', 'build-all.sh'], text=True).strip()
assert hosted['linux_conclusion'] == 'success'
for name, digest in content['artifacts'].items():
    assert sha(stage / name) == digest
for name, digest in read('DELIVERABLE_BASELINE.json').items():
    assert sha(public / name) == digest

dark = read('dark100/TEARDOWN.json')
light = read('light-miller100/TEARDOWN.json')
gui = read('dark100/GUI_SMOKE.json')
view = read('light-miller100/VIEW_OBSERVATION.json')
for observation, theme, mode in [(dark, 'Dark', 'List'), (light, 'Light', 'Miller')]:
    assert observation['normal_app_exit'] and observation['profile_removed']
    assert observation['persisted_config']['theme'] == theme
    assert observation['persisted_config']['default_view_mode'] == mode
    assert all(item['exit'] is not None for item in observation['owned_processes_reaped'])
    assert observation['artifact_sha256'] == content['artifacts']['Ottrin-0.1.0-x86_64.AppImage']
assert gui['status'] == 'pass' and gui['artifact_sha256'] == view['artifact_sha256'] == dark['artifact_sha256']
index_sha = sha(extract / 'squashfs-root/usr/bin/ottrin-indexd')
index = read('packaged-daemon/PROFILE.json')
cancel = read('packaged-cancel/PROFILE.json')
assert index['status'] == cancel['status'] == 'pass'
assert index_sha == index['binary_sha256'] == cancel['binary_sha256']
assert index['fixture_removed'] and cancel['fixture_removed'] and cancel['cached_generation_preserved']
assert index['personal_daemon_pid_before'] == index['personal_daemon_pid_after']
assert cancel['personal_daemon_pid_before'] == cancel['personal_daemon_pid_after']
assert index['first_query_ready_seconds'] < 3
html = read('PACKAGED_HTML.json')
assert html['status'] == 'pass'
assert html['binary_sha256'] == sha(extract / 'squashfs-root/usr/libexec/ottrin/ottrin-html-render')
previous = json.loads((public / 'LINUX_PACKAGES_ACCEPTANCE.json').read_text())
(evidence / 'PREVIOUS_LINUX_PACKAGES_ACCEPTANCE.json').write_text(json.dumps(previous, indent=2) + '\n')

for name, digest in content['artifacts'].items():
    subprocess.run(['bash', '-c', 'set -euo pipefail; source "$1"; ottrin_publish_artifact "$2" "$3"',
                    'publish', str(root / 'packaging/package-lifecycle.sh'), str(stage / name), str(public / name)], check=True)
    for suffix in ['.sha256', '.provenance.json']:
        path = stage / (name + suffix)
        if path.exists():
            shutil.move(path, public / path.name)
    assert sha(public / name) == digest

receipt = {
    'status': 'pass within four-format contents, scoped native AppImage and exact index-daemon acceptance',
    'source_revision': build['source_revision'], 'artifacts': content['artifacts'],
    'evidence': '.hermes/evidence/linux-filename-index-package-current',
    'checks': content['checks'] + [
        'Actual AppImage Dark/List100 startup; all 256x256 native ARGB icon pixels match assets/icon.png',
        'Read-only VCF Quick Inspector closes with Space and preserves its original bytes',
        'Actual AppImage Light/Miller100 launch displays columns, not a List table',
        'Both normal product closes exit zero and preserve their theme/view preferences',
        'Exact packaged index ELF passes the121056-item private-cache startup/reconciliation and active-scan cancellation checks',
        'Direct packaged HTML helper renders expected pixels and refuses document script on the unrestricted host',
        'Hosted complete Linux workspace, strict lint, release and production HTML setup/confinement/teardown pass'],
    'indexd_binary_sha256': index_sha,
    'indexd_resource_evidence': '.hermes/evidence/linux-filename-index-package-current/packaged-daemon/PROFILE.json',
    'indexd_resource_scope': 'Exact packaged daemon in private121056-item cache copy: startup CPU15.03s, reconciliation CPU9.50s/elapsed11.09s, saved index ready in about1s. Prior exact daemon comparison CPU22.72s startup/16.48s reconcile. Active-scan shutdown0.368s preserves prior active generation and removes staging. Single comparison, no new personal-service idle or memory claim.',
    'indexd_resource_details': {'startup': index, 'active_scan_shutdown': cancel},
    'html_binary_sha256': html['binary_sha256'], 'hosted_source_evidence': hosted,
    'html_os_policy_limit': 'The hosted OS permits unconfined profile changes when its separate restriction is 0; no prevention of an OS-authorized transition or global sysctl change is claimed.',
    'scope': 'Exact package contents, software X11 Dark/List100 and Light/Miller100, unchanged read-only VCF and direct packaged HTML rendering. No new broad operation/appearance matrix, fresh distro lifecycle, Office/HTML main-GUI, physical desktop, Windows, signing or final owner acceptance. Earlier receipts retain their own hashes and scopes.',
    'personal_setup': 'Optional personal HTML helper installation remains owner setup; no administrator prompt or root installation was attempted.',
    'cleanup': 'Private GUI and daemon profiles/processes, completed staging/extraction are removed. Prior root-owned cores remain on the waiting list; personal service remains on its prior package pending the separately authorized restart.'
}
(public / 'LINUX_PACKAGES_ACCEPTANCE.json').write_text(json.dumps(receipt, indent=2) + '\n')
(evidence / 'ACCEPTANCE.json').write_text(json.dumps(receipt, indent=2) + '\n')
assert not any(stage.iterdir())
stage.rmdir()
shutil.rmtree(extract)
for folder in evidence.rglob('__pycache__'):
    shutil.rmtree(folder)
for path in evidence.rglob('*.log'):
    if path.stat().st_size > 65536:
        path.with_suffix(path.suffix + '.gz').write_bytes(gzip.compress(path.read_bytes(), mtime=0))
        path.unlink()
final = {'status': 'pass', 'superseded_artifacts': read('DELIVERABLE_BASELINE.json'), 'stage_removed': not stage.exists(), 'extract_removed': not extract.exists(),
         'profiles_removed': dark['profile_removed'] and light['profile_removed'],
         'normal_app_exits': dark['normal_app_exit'] and light['normal_app_exit'],
         'rpm_wrappers_removed': not any(evidence.glob('rpm-bin-*')),
         'public_artifacts': content['artifacts'], 'publisher_sha256': sha(root / 'packaging/package-lifecycle.sh')}
assert final['rpm_wrappers_removed']
(evidence / 'FINAL_TEARDOWN.json').write_text(json.dumps(final, indent=2) + '\n')
for name in ['BUILD_SOURCE.json', 'EXTRACTION_OWNER.json']:
    owner = read(name)
    owner.update(pending_consumers=[], cleanup_complete=True,
                 delivery_status='Accepted and promoted; task staging and extraction removed')
    (evidence / name).write_text(json.dumps(owner, indent=2) + '\n')
print(json.dumps(final, indent=2))
