"""Promote the exact accepted Windows cohort, with rollback until publication."""
from pathlib import Path
import hashlib
import json
import shutil
import subprocess

ROOT = Path.cwd()
EVIDENCE = Path(__file__).resolve().parent
STAGE = ROOT / '.hermes/packaging-tmp/.windows-picker-stage'
DEST = ROOT / 'dist/windows-candidate'
AUTHORITY = ROOT / 'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json'
RETIRED = ROOT / '.hermes/packaging-tmp/.windows-theme-retired'


def read(path):
    return json.loads(path.read_text())


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def passed(name):
    value = read(EVIDENCE / name)
    assert value['status'].startswith('pass'), name
    return value


def unused(paths):
    # fuser argument length stays bounded even for complete payload trees.
    for start in range(0, len(paths), 50):
        result = subprocess.run(['fuser', *map(str, paths[start:start+50])], capture_output=True)
        assert result.returncode == 1, result.stderr.decode(errors='replace')


build = read(EVIDENCE / 'windows/BUILD_SOURCE.json')
source = build['source_revision']
assert Path(build['stage']) == STAGE
host = passed('HOST_PAYLOAD.json')
assembly = passed('ASSEMBLY_ACCEPTANCE.json')
native = passed('WINDOWS_NATIVE_ACCEPTANCE.json')
runtime = passed('MSIX_RUNTIME_ACCEPTANCE.json')
upgrade = passed('MSIX_UPGRADE_ACCEPTANCE.json')
daemon = passed('WINDOWS_DAEMON_RUNTIME.json')
assert all(value['source_revision'] == source for value in [host, assembly, native, runtime, upgrade, daemon])
ci = read(EVIDENCE / 'CI_CURRENT.json')
assert ci['headSha'] == source and ci['status'] == 'completed' and ci['conclusion'] == 'success'
assert len(ci['jobs']) == 4 and all(job['conclusion'] == 'success' for job in ci['jobs'])
artifacts = dict(host['artifacts'])
artifacts[Path(assembly['artifact']).name] = assembly['artifact_sha256']
assert len(artifacts) == 3
assert native['artifact_sha256'] == artifacts['Ottrin-0.1.0-windows-x64-setup.exe']
assert runtime['package']['sha256'] == upgrade['artifact_sha256'] == assembly['artifact_sha256']
assert native['case_count'] == 9 and native['preview_action_count'] == 8
assert {case['startup_enabled'] for case in upgrade['cases']} == {True, False}
for value in [native, runtime]:
    for capture, digest in value['original_captures_directly_reviewed'].items():
        assert sha(ROOT / capture) == digest
for value in [native, runtime, *upgrade['cases']]:
    assert value['cleanup']['domain_removed'] and value['cleanup']['fixture_removed']
    assert value['cleanup']['retained_pending_volume'] is None
assert daemon['protocol_shutdowns'] == 2 and daemon['private_state_removed']
assert daemon['idle']['cpu_one_core_percent'] < 1
payload = read(EVIDENCE / 'PAYLOAD.json')
new_payload = STAGE / 'windows-payload/Ottrin-0.1.0-windows-x64'
assert len(payload) == host['payload_files'] == assembly['native_payload_files'] == 58
assert {str(p.relative_to(new_payload)) for p in new_payload.rglob('*') if p.is_file()} == set(payload)
assert all(sha(new_payload / name) == digest for name, digest in payload.items())
assert sha(new_payload / 'bin/ottrin-indexd.exe') == daemon['binary_sha256']
assert (new_payload / 'SOURCE_REVISION.txt').read_text() == f'Ottrin source revision\nVersion: 0.1.0\nGit revision: {source}\nWorking tree: clean\nTracked diff SHA-256: none\n'
old_bytes = AUTHORITY.read_bytes()
assert old_bytes == (EVIDENCE / 'UNCHANGED_WINDOWS_CANDIDATE_ACCEPTANCE.json').read_bytes()
old = read(AUTHORITY)
assert upgrade['previous']['sha256'] == old['artifacts']['Ottrin-0.1.0-windows-x64-development.msix']
for name, digest in artifacts.items():
    assert sha(STAGE / name) == digest and sha(DEST / name) == old['artifacts'][name]
    assert (STAGE / (name + '.sha256')).read_text().split()[0] == digest
allowed = set(artifacts) | {name + '.sha256' for name in artifacts} | {'Ottrin-0.1.0-windows-x64-development.msix.provenance.json', 'windows-payload'}
assert {p.name for p in STAGE.iterdir()} == allowed
assert {p.name for p in DEST.iterdir()} == allowed | {'PREVIEW_STATUS.md'}
files = [p for folder in [STAGE, DEST] for p in folder.rglob('*') if p.is_file()]
assert not any(p.is_symlink() for folder in [STAGE, DEST] for p in folder.rglob('*'))
unused(files)
# Current protected VMs stay off; no task-owned clone or package consumer remains.
domains = subprocess.check_output(['virsh', '-c', 'qemu:///system', 'list', '--all', '--name'], text=True).split()
assert set(domains) == {'win11ltsc', 'ottrin-copy-cleanup-qa', 'ottrin-index-qa', 'ottrin-qa-20261011'}
linux_authority = ROOT / 'dist/packages/LINUX_PACKAGES_ACCEPTANCE.json'
linux_bytes = linux_authority.read_bytes()
linux = read(linux_authority)
assert all(sha(ROOT / 'dist/packages' / name) == digest for name, digest in linux['artifacts'].items())
assert not RETIRED.exists()
temporary = AUTHORITY.with_suffix('.json.next')
assert not temporary.exists()
retired_files = [{'path': str(p.relative_to(DEST)), 'bytes': p.stat().st_size, 'sha256': sha(p), 'links_before': p.stat().st_nlink} for p in DEST.rglob('*') if p.is_file()]
manifest = {
    'status': 'pass scoped shared-widget Windows preview delivery; full roadmap incomplete',
    'source_commit': source, 'working_tree_at_build': 'clean', 'candidate': str(DEST),
    'artifacts': artifacts, 'payload_files': 58,
    'evidence_directory': str(EVIDENCE.relative_to(ROOT)),
    'hosted_source': {'run': ci['run_id'], 'tested_source_revision': source, 'all_four_jobs_success': True},
    'host_content': str((EVIDENCE / 'HOST_PAYLOAD.json').relative_to(ROOT)),
    'current_native_fields': str((EVIDENCE / 'WINDOWS_NATIVE_ACCEPTANCE.json').relative_to(ROOT)),
    'daemon_runtime': str((EVIDENCE / 'WINDOWS_DAEMON_RUNTIME.json').relative_to(ROOT)),
    'native_msix': {key: str((EVIDENCE / name).relative_to(ROOT)) for key, name in [('current_assembly', 'ASSEMBLY_ACCEPTANCE.json'), ('fresh_runtime', 'MSIX_RUNTIME_ACCEPTANCE.json'), ('enabled_disabled_upgrade', 'MSIX_UPGRADE_ACCEPTANCE.json')]},
    'native_msi_rollback': old['native_msi_rollback'],
    'source_only_followup': {'source': '50a78015b8e23b6da558a12cc5d8536ce538e1d2', 'scope': 'Breadcrumb directory classification source-tested, not included in this458 package'},
    'limits': ['NSIS Dark144DPI Native/OpenGL settings and preview selection only; not every field/theme/scale', 'MSIX17 unsigned development deployment, fresh runtime and same-identity16-to17 updates; no Store/signing/physical acceptance', 'Previous full Services matrices retain original source/artifact scopes', 'Full U08 audit, Linux local-seat elevation, remaining picker/folder integration, MSI rollback and owner acceptance remain open'],
    'public_release_published': False,
}
status_text = '# Current Windows preview\n\nRuntime source `' + source + '`. ZIP/NSIS and unsigned development MSIX0.1.0.17. Exact NSIS passes nine settings cases and eight preview actions in Native/OpenGL at144DPI. Fresh MSIX runtime, real reboot/login, removal and same-identity16-to17 updates with startup enabled and disabled pass. Preferences/index identity are preserved; disabled startup remains stopped through180seconds after login. These are scoped software-desktop results. Full interface/theme audit, remaining platform integration, MSI rollback, physical graphics, signing/Store and owner acceptance remain open. Source50a breadcrumb follow-up is tested but not packaged. See ../WINDOWS_CANDIDATE_ACCEPTANCE.json.\n'
RETIRED.mkdir()
undo = []
def move(a, b):
    a.replace(b)
    undo.append((b, a))
try:
    for path in list(DEST.iterdir()):
        move(path, RETIRED / path.name)
    for path in list(STAGE.iterdir()):
        move(path, DEST / path.name)
    (DEST / 'PREVIEW_STATUS.md').write_text(status_text)
    assert all(sha(DEST / name) == digest for name, digest in artifacts.items())
    assert all(sha(DEST / 'windows-payload/Ottrin-0.1.0-windows-x64' / name) == digest for name, digest in payload.items())
    assert linux_authority.read_bytes() == linux_bytes
    temporary.write_text(json.dumps(manifest, indent=2) + '\n')
    temporary.replace(AUTHORITY)
except BaseException:
    temporary.unlink(missing_ok=True)
    (DEST / 'PREVIEW_STATUS.md').unlink(missing_ok=True)
    for current, previous in reversed(undo):
        current.replace(previous)
    if not any(RETIRED.iterdir()):
        RETIRED.rmdir()
    raise
# Authority now names verified stable bytes; old input is no longer needed by any fixture.
unused([p for p in RETIRED.rglob('*') if p.is_file()])
shutil.rmtree(RETIRED)
STAGE.rmdir()
receipt = {'status': 'pass promoted exact verified Windows cohort', 'source_revision': source, 'artifacts': artifacts, 'superseded_files_removed': retired_files, 'consumed_stage_removed': str(STAGE), 'linux_artifacts_and_authority_preserved': True, 'held_msi_unchanged': True, 'retained': str(DEST)}
(EVIDENCE / 'WINDOWS_PROMOTION.json').write_text(json.dumps(receipt, indent=2) + '\n')
build.update(cleanup_complete=True, pending_consumers=[], delivery_status='Promoted accepted ZIP/NSIS/MSIX17 to stable Windows candidate; consumed stage removed', stable_path=str(DEST), scope='Shared theme widgets/Miller rows, exact native NSIS and fresh/update/login MSIX17 scopes in delivery authority; held MSI unchanged')
(EVIDENCE / 'windows/BUILD_SOURCE.json').write_text(json.dumps(build, indent=2) + '\n')
print(json.dumps({key: value for key, value in receipt.items() if key != 'superseded_files_removed'}, indent=2))
