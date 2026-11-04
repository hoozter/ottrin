"""Publish the accepted seven-format idle-observation cohort at stable paths."""
from pathlib import Path
import hashlib
import json
import shutil
import subprocess

r = Path.cwd()
e = Path(__file__).resolve().parent
read = lambda p: json.loads(p.read_text())
def sha(p):
    with p.open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()
def write(p, value):
    temp = p.with_suffix(p.suffix + '.next')
    temp.write_text(json.dumps(value, indent=2) + '\n')
    temp.replace(p)
def unused(files):
    assert files and all(p.is_file() and not p.is_symlink() and p.stat().st_nlink == 1 for p in files)
    assert subprocess.run(['fuser', *map(str, files)], capture_output=True).returncode == 1
def passed(p):
    v = read(p)
    assert v['status'].startswith('pass'), p
    return v

source = read(e / 'BUILD_SOURCE.json')['source_revision']
ci = read(e / 'CI_CURRENT.json')
assert ci['headSha'] == source and ci['status'] == 'completed' and ci['conclusion'] == 'success'
assert {j['name'] for j in ci['jobs']} == {'verify (ubuntu-latest)', 'verify (windows-latest)', 'search-performance (ubuntu-latest)', 'search-performance (windows-latest)'}
assert all(j['conclusion'] == 'success' for j in ci['jobs'])
scope = passed(e / 'SOURCE_SCOPE.json')
assert scope['candidate_source'] == source
subprocess.run(['git', 'diff', '--exit-code', source, '--', '.', ':!.hermes/**', ':!docs/ROADMAP.md', ':!dist/**'], check=True, stdout=subprocess.PIPE)
linux = passed(e / 'LINUX_NATIVE_ACCEPTANCE.json')
windows = passed(e / 'WINDOWS_NATIVE_ACCEPTANCE.json')
assert linux['source_revision'] == windows['source_revision'] == source
assert windows['cases'] == 4 and windows['full_window_captures'] == 6
assert linux['cases']['candidate']['idle']['cpu_one_core_percent'] < 2
assert len(windows['gui_idle_baseline']) == 2
assert all(v['cpu_one_core_percent'] < 2 and v['seconds'] >= 30 for v in windows['gui_idle_baseline'])
for p in ['FULL_LINUX_PAYLOAD_COMPARISON.json', 'FULL_WINDOWS_PAYLOAD_COMPARISON.json']:
    assert passed(e / p)['candidate_source'] == source
reuse = passed(e / 'LINUX_DAEMON_REUSE.json')
prior = r / reuse['previous_runtime_receipt']
assert reuse['source_revision'] == source and sha(prior) == reuse['previous_receipt_sha256']
assert passed(e / 'LINUX_INDEX_ELF_COMPARISON.json')['mapped_image_equal']
daemon = passed(e / 'WINDOWS_DAEMON_RUNTIME.json')
assert daemon['source_revision'] == source and daemon['binary_sha256'] == read(e / 'PAYLOAD.json')['bin/ottrin-indexd.exe']
assert daemon['idle']['seconds'] >= 30 and daemon['idle']['cpu_one_core_percent'] < 1
assert daemon['source_and_preferences_preserved'] and daemon['private_state_removed']
assert daemon['protocol_shutdowns'] == 2 and daemon['watcher_mutations'] == ['create', 'rename', 'delete']
assembly = passed(e / 'ASSEMBLY_ACCEPTANCE.json')
assert assembly['source_revision'] == source and assembly['version'] == '0.1.0.14'
for label in ['native', 'assembly']:
    owner = read(e / label / 'OWNER.json')
    assert owner['cleanup']['domain_removed'] and owner['cleanup']['fixture_removed']
    assert len(owner['cleanup']['owned_volumes_removed']) == 2
    assert subprocess.run(['virsh', '-c', 'qemu:///system', 'domuuid', owner['domain']], capture_output=True).returncode != 0
assert not (e / 'source.zip').exists()
assert not (r / '.hermes/packaging-tmp/.linux-desktop-extract').exists()
lin_stage = Path(read(e / 'BUILD_SOURCE.json')['stage'])
win_stage = Path(read(e / 'windows/BUILD_SOURCE.json')['stage'])
assert lin_stage == r / '.hermes/packaging-tmp/.linux-native-desktop-stage'
assert win_stage == r / '.hermes/packaging-tmp/.windows-picker-stage'
lin_dest, win_dest = r / 'dist/packages', r / 'dist/windows-candidate'
lin_authority, win_authority = lin_dest / 'LINUX_PACKAGES_ACCEPTANCE.json', r / 'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json'
old_lin, old_win = read(lin_authority), read(win_authority)
assert (e / 'PREVIOUS_LINUX_PACKAGES_ACCEPTANCE.json').read_bytes() == lin_authority.read_bytes()
assert (e / 'PREVIOUS_WINDOWS_CANDIDATE_ACCEPTANCE.json').read_bytes() == win_authority.read_bytes()
content = passed(e / 'PACKAGE_CONTENT_ACCEPTANCE.json')
host = passed(e / 'HOST_PAYLOAD.json')
lin_artifacts = content['artifacts']
win_artifacts = host['artifacts'] | {Path(assembly['artifact']).name: assembly['artifact_sha256']}
assert len(lin_artifacts) == 4 and len(win_artifacts) == 3
for dest, stage, old, new in [(lin_dest, lin_stage, old_lin['artifacts'], lin_artifacts), (win_dest, win_stage, old_win['artifacts'], win_artifacts)]:
    for name, digest in old.items():
        assert sha(dest / name) == digest
    for name, digest in new.items():
        assert sha(stage / name) == digest
        assert (stage / (name + '.sha256')).read_text().split()[0] == digest
payload = read(e / 'PAYLOAD.json')
payload_root = 'windows-payload/Ottrin-0.1.0-windows-x64'
manifest = lambda root: {str(p.relative_to(root)): sha(p) for p in root.rglob('*') if p.is_file()}
assert manifest(win_stage / payload_root) == payload and len(payload) == 58
assert len(manifest(win_dest / payload_root)) == 58
held_path = r / '.hermes/evidence/windows-msi-package-control-current/input/DIAGNOSTIC_INPUT.json'
held_bytes = held_path.read_bytes()
held = read(held_path)
assert sha(Path(held['artifact'])) == held['sha256']
stage_files = [p for p in lin_stage.rglob('*') if p.is_file()]
assert {str(p.relative_to(lin_stage)) for p in stage_files} <= {n + s for n in lin_artifacts for s in ['', '.sha256', '.provenance.json']}
unused(stage_files)
old_win_files = [p for p in win_dest.rglob('*') if p.is_file()]
known = {str(Path(payload_root) / n) for n in manifest(win_dest / payload_root)} | {n + s for n in old_win['artifacts'] for s in ['', '.sha256', '.provenance.json']} | {'PREVIEW_STATUS.md'}
assert {str(p.relative_to(win_dest)) for p in old_win_files} <= known
unused(old_win_files)
(win_stage / 'PREVIEW_STATUS.md').write_text('# Current Windows preview\n\nRuntime source `' + source + '`. ZIP/NSIS and unsigned development MSIX0.1.0.14. Both Native/OpenGL pass scoped Dark144DPI GUI idle CPU, automatic native index-count updates, OS drive insertion, normal close and supported uninstall. Six current full-window captures reviewed. MSIX14 native assembly/unpack matches all58payload files; deployment/update/login acceptance remains historical. Earlier appearance/preview/picker/folder/service checks retain their original source/artifact scopes. Full roadmap, MSI rollback, physical graphics, signing/Store and owner acceptance remain open. See ../WINDOWS_CANDIDATE_ACCEPTANCE.json.\n')
new_win_files = [p for p in win_stage.rglob('*') if p.is_file()]
known = {str(Path(payload_root) / n) for n in payload} | {n + s for n in win_artifacts for s in ['', '.sha256', '.provenance.json']} | {'PREVIEW_STATUS.md'}
assert {str(p.relative_to(win_stage)) for p in new_win_files} <= known
unused(new_win_files)
unused([lin_dest / n for n in old_lin['artifacts']])
retired = r / '.hermes/packaging-tmp/.windows-candidate-retired'
assert not retired.exists()
write(e / 'PROMOTION_INTENT.json', {'source': source, 'linux_from': str(lin_stage), 'windows_from': str(win_stage), 'windows_retired': str(retired), 'held_msi_sha256': held['sha256'], 'all_native_consumers_terminal': True})
common = {'evidence': str(e.relative_to(r)), 'source_scope': str((e / 'SOURCE_SCOPE.json').relative_to(r)), 'hosted_source': {'run': ci['databaseId'], 'tested_source_revision': source, 'all_four_jobs_success': True}, 'limits': ['Bounded private software-renderer idle samples; no universal zero CPU, sustained-use or physical graphics acceptance', 'Current checks exercise changed observation responsibilities; other receipts retain original hashes/scopes', 'Local unsigned preview; full roadmap, MSI rollback, signing/Store and final owner acceptance remain open']}
lin_receipt = common | {'status': 'pass scoped current Linux idle-observation preview; full roadmap incomplete', 'source_revision': source, 'artifacts': lin_artifacts, 'checks': content['checks'], 'current_native_fields': str((e / 'LINUX_NATIVE_ACCEPTANCE.json').relative_to(r)), 'full_payload_comparison': str((e / 'FULL_LINUX_PAYLOAD_COMPARISON.json').relative_to(r)), 'daemon_runtime_reuse': str((e / 'LINUX_DAEMON_REUSE.json').relative_to(r)), 'unchanged_responsibility_basis': {'previous_source': old_lin['source_revision'], 'previous_receipt': str((e / 'PREVIOUS_LINUX_PACKAGES_ACCEPTANCE.json').relative_to(r)), 'scope': scope['shipping_behavior']}}
for name, digest in lin_artifacts.items():
    subprocess.run(['bash', '-c', 'set -euo pipefail;source "$1";ottrin_publish_artifact "$2" "$3"', 'publish', str(r / 'packaging/package-lifecycle.sh'), str(lin_stage / name), str(lin_dest / name)], check=True)
    for suffix in ['.sha256', '.provenance.json']:
        p = lin_stage / (name + suffix)
        if p.exists():
            shutil.move(p, lin_dest / p.name)
    assert sha(lin_dest / name) == digest
assert not any(lin_stage.iterdir())
lin_stage.rmdir()
write(lin_authority, lin_receipt)
write(e / 'LINUX_DELIVERY.json', lin_receipt)
win_dest.rename(retired)
try:
    win_stage.rename(win_dest)
except BaseException:
    retired.rename(win_dest)
    raise
assert manifest(win_dest / payload_root) == payload
for name, digest in win_artifacts.items():
    assert sha(win_dest / name) == digest
assert held_path.read_bytes() == held_bytes and sha(Path(held['artifact'])) == held['sha256']
win_receipt = common | {'status': 'pass scoped current Windows idle-observation preview; MSI release held separately', 'source_commit': source, 'working_tree_at_build': 'clean', 'candidate': str(win_dest), 'artifacts': win_artifacts, 'payload_files': 58, 'evidence_directory': str(e.relative_to(r)), 'host_content': 'HOST_PAYLOAD.json', 'current_native_fields': str((e / 'WINDOWS_NATIVE_ACCEPTANCE.json').relative_to(r)), 'gui_idle_baseline': windows['gui_idle_baseline'], 'full_payload_comparison': str((e / 'FULL_WINDOWS_PAYLOAD_COMPARISON.json').relative_to(r)), 'daemon_runtime': str((e / 'WINDOWS_DAEMON_RUNTIME.json').relative_to(r)), 'native_msix': {'current_assembly': str((e / 'ASSEMBLY_ACCEPTANCE.json').relative_to(r)), 'limit': 'MSIX14 assembly/unpack only; deployment/update/login/removal acceptance retains historical source/artifact scopes'}, 'unchanged_responsibility_basis': {'previous_source': old_win['source_commit'], 'previous_receipt': str((e / 'PREVIOUS_WINDOWS_CANDIDATE_ACCEPTANCE.json').relative_to(r)), 'scope': scope['shipping_behavior']}, 'native_msi_rollback': old_win['native_msi_rollback'], 'public_release_published': False}
write(win_authority, win_receipt)
removed = [{'path': str(p.relative_to(retired)), 'bytes': p.stat().st_size, 'sha256': sha(p)} for p in retired.rglob('*') if p.is_file()]
unused([p for p in retired.rglob('*') if p.is_file()])
shutil.rmtree(retired)
assembly['assembled_artifact_path'] = assembly['artifact']
assembly['artifact'] = str(win_dest / Path(assembly['artifact']).name)
write(e / 'ASSEMBLY_ACCEPTANCE.json', assembly)
write(e / 'WINDOWS_DELIVERY.json', {'status': 'pass current Windows preview promoted', 'source_revision': source, 'published': str(win_dest), 'artifacts': win_artifacts, 'all_58_payload_hashes_verified_after_promotion': True, 'held_msi_preserved_sha256': held['sha256'], 'removed_superseded_files': removed, 'removed_superseded_logical_bytes': sum(p['bytes'] for p in removed), 'staging_removed': not win_stage.exists(), 'retired_directory_removed': not retired.exists()})
for path in [e / 'BUILD_SOURCE.json', e / 'windows/BUILD_SOURCE.json']:
    v = read(path)
    v.update(pending_consumers=[], cleanup_complete=True, delivery_status='Verified current idle-observation cohort promoted; staging removed')
    write(path, v)
print('Published seven exact accepted artifacts and retired superseded output')
