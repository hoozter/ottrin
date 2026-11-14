"""Verify and promote the picker density Windows candidate as one directory."""
import argparse
import hashlib
import json
import shutil
import subprocess
import zipfile
from pathlib import Path

root = Path.cwd()
evidence = Path(__file__).resolve().parent
stage = root / '.hermes/packaging-tmp/.windows-picker-density-stage'
retired = stage.with_name('.windows-candidate-retired')
dest = root / 'dist/windows-candidate'
authority = root / 'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json'
pending = authority.with_suffix('.next')
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--publish', action='store_true')
args = parser.parse_args()

def read(path):
    return json.loads(path.read_text())

def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()

def save(path, value):
    path.write_text(json.dumps(value, indent=2) + '\n')

def unused(files):
    assert all(p.is_file() and not p.is_symlink() and p.stat().st_nlink == 1 for p in files)
    for start in range(0, len(files), 30):
        assert subprocess.run(['fuser', *map(str, files[start:start + 30])], capture_output=True).returncode == 1

old = read(authority)
old_bytes = authority.read_bytes()
host = read(evidence / 'HOST_PAYLOAD.json')
assembly = read(evidence / 'ASSEMBLY_ACCEPTANCE.json')
payload = host['payload_hashes']
picker = read(evidence / 'ACCEPTANCE.json')
runtime_case = 'runtime39-upgrade'
runtime = read(evidence / runtime_case / 'ACCEPTANCE.json')
source = host['source_revision']
assert source == assembly['source_revision'] == picker['input']['source_revision'] == runtime['input']['source_revision']
assert len(payload) == host['payload_files'] == 59 and host['installer_matches_portable']
assert assembly['version'] == '0.1.0.39'
assert picker['status']=='pass native portable and installed List density and picker preferences'
assert picker['preferences_and_fixture_preserved'] and picker['no_index'] and picker['grid_and_hidden_preserved']
assert picker['artifact_hashes'] == host['artifacts'] and picker['installed_uninstall_passed']
assert set(picker['list_density']) == {'portable', 'installed'}
for cases in picker['list_density'].values():
    compact, large = cases
    assert [compact['scale'], large['scale']] == [0.9, 1.35]
    assert large['pitch'] - compact['pitch'] >= 8
    assert compact['opened_expected_file'] and large['opened_expected_file']
assert [case['theme'] for case in picker['installed_theme']] == ['Dark', 'Light']
assert all(case['physical_cancel'] and case['preview_menu'] for case in picker['installed_theme'])
assert [row['case'] for row in picker['cases']] == ['portable-grid', 'installed-hidden', 'portable-untouched']
assert all(row['normal_cancel'] for row in picker['cases'])
assert picker['binary_sha256'] == payload['bin/ottrin.exe']
assert picker['cleanup']['domain_removed'] and picker['cleanup']['fixture_removed']
assert runtime['administrator_copy_availability']['notice_visible']
assert runtime['administrator_copy_availability']['helper_sha256'] == payload['bin/ottrin-priv-helper.exe']
assert runtime['input']['sha256'] == assembly['artifact_sha256'] and runtime['input']['startup_enabled']
assert runtime['input']['previous']['sha256'] == old['artifacts'][Path(assembly['artifact']).name]
for receipt in ('WINDOWS_PICKER_SPACE.json', 'MSIX_ASSEMBLY_SPACE.json', 'MSIX_RUNTIME_SPACE.json'):
    value = read(evidence / receipt)
    assert value['fixture_exit'] == 0 and value['compression_stage_removed']
for case in ('windows-native', 'assembly39', runtime_case):
    owner = read(evidence / case / 'OWNER.json')
    assert owner['cleanup']['domain_removed'] and owner['cleanup']['fixture_removed']
    assert subprocess.run(['virsh', '-c', 'qemu:///system', 'domuuid', owner['domain']], capture_output=True).returncode != 0

ci = json.loads(subprocess.check_output(['gh', 'run', 'view', '42790815680', '--json', 'status,conclusion,headSha,jobs'], text=True))
assert ci['status'] == 'completed' and ci['conclusion'] == 'success'
assert {job['name'] for job in ci['jobs'] if job['conclusion'] == 'success'} == {
    'verify (ubuntu-latest)', 'verify (windows-latest)', 'search-performance (ubuntu-latest)', 'search-performance (windows-latest)'}
subprocess.run(['git', 'diff', '--quiet', ci['headSha'], source, '--', '.', ':!.hermes/evidence/**', ':!dist/**', ':!docs/ROADMAP.md'], check=True)
artifacts = host['artifacts'] | {Path(assembly['artifact']).name: assembly['artifact_sha256']}
for name, digest in artifacts.items():
    assert sha(stage / name) == digest and (stage / (name + '.sha256')).read_text().split()[0] == digest
for name, digest in old['artifacts'].items():
    assert sha(dest / name) == digest
old_payload = read(root / old['evidence_directory'] / old['host_content'])['payload_hashes']
old_payload_dir = dest / 'windows-payload/Ottrin-0.1.0-windows-x64'
assert len(old_payload) == 59 and all(sha(old_payload_dir / name) == digest for name, digest in old_payload.items())
old_files = [p for p in dest.rglob('*') if p.is_file()]
allowed_old = {old_payload_dir / name for name in old_payload}
allowed_old.update(dest / (name + suffix) for name in old['artifacts'] for suffix in ('', '.sha256', '.provenance.json'))
allowed_old.add(dest / 'PREVIEW_STATUS.md')
assert set(old_files) <= allowed_old and not any(p.is_symlink() for p in dest.rglob('*'))
allowed_stage = {stage / (name + suffix) for name in artifacts for suffix in ('', '.sha256', '.provenance.json')}
assert set(stage.iterdir()) <= allowed_stage
unused(old_files)
unused([p for p in stage.rglob('*') if p.is_file()])
assert not retired.exists() and not pending.exists() and stage.stat().st_dev == dest.stat().st_dev
held = root / old['native_msi_rollback']['diagnostic_input']
held_input = read(held)
held_path = Path(held_input['artifact'])
linux = root / 'dist/packages/LINUX_PACKAGES_ACCEPTANCE.json'
linux_bytes = linux.read_bytes()
linux_receipt = read(linux)
assert all(sha(linux.parent / name) == digest for name, digest in linux_receipt['artifacts'].items())
assert old['source_commit'] == 'f62ec5e4319514e699112a3442da630747cd2839'
assert all(old_payload[name]==digest for name,digest in payload.items() if name.startswith('bin/') and name != 'bin/ottrin.exe')
assert sha(held_path) == held_input['sha256']
portable_name, = [name for name in artifacts if name.endswith('.zip')]
prefix = 'Ottrin-0.1.0-windows-x64/'
payload_dir = stage / 'windows-payload'
assert not payload_dir.exists()
with zipfile.ZipFile(stage / portable_name) as archive:
    members = [i.filename for i in archive.infolist() if not i.is_dir()]
    assert len(members) == len(payload) and set(members) == {prefix + name for name in payload}
    assert all(not Path(name).is_absolute() and '..' not in Path(name).parts and '\\' not in name for name in payload)
    assert all(hashlib.sha256(archive.read(prefix + name)).hexdigest() == digest for name, digest in payload.items())
print('Windows promotion preflight passed')
if not args.publish:
    raise SystemExit(0)
subprocess.run(['git', 'merge-base', '--is-ancestor', source, 'HEAD'], check=True)
subprocess.run(['git', 'diff', '--quiet', source, 'HEAD', '--', '.', ':!.hermes/evidence/**', ':!dist/**', ':!docs/ROADMAP.md'], check=True)

preview = stage / 'PREVIEW_STATUS.md'
preview_text = f"""# Current Windows preview

Source `{source[:8]}` is available as a portable ZIP, an NSIS installer and
unsigned development MSIX 39. List spacing changed with the saved density
in the ZIP and installed picker. Both opened the exact file clicked. The
installed Theme Editor passed preview-menu and pointer Cancel in both variants.
Three overlapping picker windows from the
portable ZIP and installed EXE preserved independent grid and hidden-file
changes. The app keeps deferred OpenGL loading. Portable cold startup
remains under investigation. Administrator Copy is available in the desktop formats.

MSIX passed upgrade from revision 38 with automatic indexing enabled, real
sign-in, Explorer folder actions and removal.

See ../WINDOWS_CANDIDATE_ACCEPTANCE.json for scope and artifact hashes.
"""
relative = str(evidence.relative_to(root))
manifest = old | dict(
    status='pass scoped Windows picker density, installed theme and MSIX upgrade',
    source_commit=source, artifacts=artifacts, evidence_directory=relative,
    hosted_source=dict(run=42790815680, tested_source_revision=ci['headSha'], all_four_jobs_success=True),
    native_picker_preferences=relative + '/ACCEPTANCE.json',
    native_picker_density_and_installed_theme=relative + '/ACCEPTANCE.json',
    daemon_runtime=relative + '/WINDOWS_DAEMON_RUNTIME.json',
    native_msix=dict(current_assembly=relative + '/ASSEMBLY_ACCEPTANCE.json',
                     current_enabled_update=relative + '/runtime39-upgrade/ACCEPTANCE.json'),
    previous_native_msix=old['native_msix'],
    source_scope=relative + '/WINDOWS_DELIVERY.json',
    host_content='HOST_PAYLOAD.json',
    limits=[
        'Software Windows guest; physical GPU, Store, signing and human acceptance remain separate.',
        'Earlier theme, Save, administrator Copy, NSIS lifecycle and default-folder receipts retain their original package scopes.',
        'MSIX exposes Explorer folder actions and preserves the default folder app.',
        'Administrator Copy is available in desktop builds. Single-user MSIX administrator Copy and ordinary MSI rollback remain held.',
    ],
)
swapped = False
try:
    with zipfile.ZipFile(stage / portable_name) as archive:
        for name in payload:
            member = prefix + name
            path = payload_dir / member
            path.parent.mkdir(parents=True, exist_ok=True)
            with archive.open(member) as source_file, path.open('xb') as target_file:
                shutil.copyfileobj(source_file, target_file)
    preview.write_text(preview_text)
    dest.rename(retired)
    try:
        stage.rename(dest)
    except BaseException:
        retired.rename(dest)
        raise
    swapped = True
    assert all(sha(dest / name) == digest for name, digest in artifacts.items())
    assert all(sha(dest / 'windows-payload' / prefix / name) == digest for name, digest in payload.items())
    assert linux.read_bytes() == linux_bytes and sha(held_path) == held_input['sha256']
    save(pending, manifest)
    pending.replace(authority)
except BaseException:
    pending.unlink(missing_ok=True)
    if swapped:
        dest.rename(stage)
        retired.rename(dest)
        authority.write_bytes(old_bytes)
    if payload_dir.exists():
        shutil.rmtree(payload_dir)
    preview.unlink(missing_ok=True)
    raise

retired_files = [p for p in retired.rglob('*') if p.is_file()]
unused(retired_files)
retired_bytes = sum(p.stat().st_size for p in retired_files)
shutil.rmtree(retired)
assembly['assembled_artifact_path'] = assembly['artifact']
assembly['artifact'] = str(dest / Path(assembly['artifact']).name)
save(evidence / 'ASSEMBLY_ACCEPTANCE.json', assembly)
save(evidence / 'WINDOWS_DELIVERY.json', {
    'status': 'pass Windows preview promoted', 'source_revision': source,
    'artifacts': artifacts, 'payload_files': len(payload), 'hosted_source': manifest['hosted_source'],
    'previous_source': old['source_commit'], 'retired_logical_bytes': retired_bytes,
    'stage_removed': not stage.exists(), 'retired_removed': not retired.exists(),
    'linux_authority_preserved': linux.read_bytes() == linux_bytes,
    'held_msi_preserved': sha(held_path) == held_input['sha256'],
})
print('Windows ZIP, NSIS and development MSIX 39 delivered')
