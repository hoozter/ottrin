"""Rehash stable delivery and verify exact ownership teardown and storage review."""
from pathlib import Path
import hashlib
import json
import subprocess

r = Path.cwd()
e = Path(__file__).resolve().parent
read = lambda p: json.loads(p.read_text())
def sha(p):
    with p.open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()
source = read(e / 'BUILD_SOURCE.json')['source_revision']
lin = read(r / 'dist/packages/LINUX_PACKAGES_ACCEPTANCE.json')
win = read(r / 'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json')
assert lin['source_revision'] == win['source_commit'] == source
artifacts = {}
for root, receipt in [(r / 'dist/packages', lin), (r / 'dist/windows-candidate', win)]:
    for name, digest in receipt['artifacts'].items():
        p = root / name
        assert sha(p) == digest
        assert (root / (name + '.sha256')).read_text().split()[0] == digest
        artifacts[str(p.relative_to(r))] = {'sha256': digest, 'bytes': p.stat().st_size, 'sidecar_matches': True}
assert len(artifacts) == 7
folder = r / 'dist/windows-candidate/windows-payload/Ottrin-0.1.0-windows-x64'
assert {str(p.relative_to(folder)): sha(p) for p in folder.rglob('*') if p.is_file()} == read(e / 'PAYLOAD.json')
assert len(read(e / 'PAYLOAD.json')) == 58
held = read(r / '.hermes/evidence/windows-msi-package-control-current/input/DIAGNOSTIC_INPUT.json')
assert sha(Path(held['artifact'])) == held['sha256']
absent = [r / '.hermes/packaging-tmp' / n for n in ['.linux-native-desktop-stage', '.windows-picker-stage', '.windows-candidate-retired', '.linux-desktop-extract', '.windows-msix-source']] + [e / 'source.zip']
assert all(not p.exists() for p in absent)
vms, profiles = [], []
for label in ['services-refresh-ambiguity', 'services-reader-sharing', 'services', 'native', 'assembly']:
    o = read(e / label / 'OWNER.json')
    assert o['cleanup']['domain_removed'] and o['cleanup']['fixture_removed']
    assert len(o['cleanup']['owned_volumes_removed']) == 2
    assert subprocess.run(['virsh', '-c', 'qemu:///system', 'domuuid', o['domain']], capture_output=True).returncode != 0
    for name in o['cleanup']['owned_volumes_removed']:
        assert subprocess.run(['virsh', '-c', 'qemu:///system', 'vol-path', '--pool', 'default', name], capture_output=True).returncode != 0
    for name in ['RESULTS.zip', 'controller.ps1', 'GUEST_PREPARATION.json', 'CONTROLLER_PROCESS.json', 'DESKTOP_SIGNAL.json', 'readiness.log', 'desktop-ready.png', 'PRIVACY_STEPS.json']:
        assert not (e / label / name).exists()
    assert not list((e / label).glob('privacy-accepted-*.png'))
    vms.append({'label': label, 'uuid': o['uuid'], 'cleanup': o['cleanup']})
    if 'private_console' in o:
        p = Path(o['private_console']['home']).parent
        assert not p.exists()
        profiles.append(str(p))
for label in ['linux-candidate']:
    t = read(e / label / 'TEARDOWN.json')
    assert t['profile_removed'] and not Path(t['profile']).exists()
    profiles.append(t['profile'])
daemon = read(e / 'LINUX_DAEMON_RUNTIME.json')
assert daemon['owned_daemon_exited'] and daemon['private_profile_removed']
profile = Path(daemon['initial_ready']['last_indexed_path']).parents[2]
assert profile.name.startswith('ot-daemon-') and not profile.exists()
profiles.append(str(profile))
domains = set(subprocess.check_output(['virsh', '-c', 'qemu:///system', 'list', '--all', '--name'], text=True).split())
assert domains == {'ottrin-copy-cleanup-qa', 'ottrin-index-qa', 'ottrin-qa-20261011', 'win11ltsc'}
assert 'identified: 0' in (e / 'incremental-final-inspection.log').read_text()
verification = {'status': 'pass exact seven-format publication and scoped owned cleanup', 'source_revision': source, 'artifacts': artifacts, 'windows_payload_files': 58, 'held_msi_preserved_sha256': held['sha256'], 'limits': ['Current native settings preservation and Windows task/login controls pass at scoped software-renderer coverage; earlier GUI idle sample retains47a source scope', 'MSIX16 assembly/unpack only; historical lifecycle retains original hashes', 'Full roadmap, MSI rollback, sustained/personal/physical acceptance and signing/Store remain open']}
(e / 'DELIVERY_VERIFICATION.json').write_text(json.dumps(verification, indent=2) + '\n')
cleanup = {'status': 'complete for current settings/service delivery task resources', 'owned_vms': vms, 'removed_private_profiles': profiles, 'removed_stages_extractions_archives': [str(p) for p in absent], 'consumed_inputs': read(e / 'CONSUMED_INPUTS_RETIRED.json'), 'superseded_windows_logical_bytes': read(e / 'WINDOWS_DELIVERY.json')['removed_superseded_logical_bytes'], 'superseded_linux': 'Replaced at stable paths through production lifecycle after exact hashes, link and open-consumer checks', 'compiler_storage_comparison': read(e / 'COMPILER_CLEANUP.json'), 'obsolete_incremental_sessions_removed': 22, 'final_obsolete_incremental_candidates': 0, 'protected_domains_retained': sorted(domains), 'remaining_task_created_storage': [{'path': str(e.parent), 'purpose': 'Compact progress/acceptance receipts, frozen producers, reviewed originals and diagnostic logs; current evidence retained'}, {'path': str(r / '.hermes/evidence/service-startup-current'), 'purpose': 'Earlier exact-source regression and retired-candidate/fixture receipts; no runtime staging or source archive retained'}, {'path': str(r / '.hermes/tools/windows-display-driver'), 'purpose': 'Small useful signed display-driver cache for native DPI QA'}, {'path': str(r / 'dist/packages'), 'purpose': 'Four latest usable Linux deliverables'}, {'path': str(r / 'dist/windows-candidate'), 'purpose': 'Latest ZIP/NSIS/MSIX16 and 58-file payload for delivery'}], 'pending_disposable_consumers': [], 'scope': 'Personal services/settings/data, current usable packages, protected VMs and useful shared compiler caches preserved.'}
(e / 'CLEANUP_FINAL.json').write_text(json.dumps(cleanup, indent=2) + '\n')
print('Verified seven artifacts, 58 Windows payloads, five VMs/ten volumes and five desktop profiles cleaned; private daemon profile separately removed')
