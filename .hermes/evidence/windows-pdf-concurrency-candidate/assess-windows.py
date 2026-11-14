from pathlib import Path
import hashlib
import json
import subprocess
import zipfile

root = Path(__file__).resolve().parent
project = root.parents[2]
case = root / 'windows-native'

def read(path):
    return json.loads(path.read_text())

def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()

result = read(case / 'GUEST_RESULT.json')
inputs = read(case / 'APPLICATION_INPUT.json')
space = read(root / 'WINDOWS_PICKER_SPACE.json')
payload = read(root / 'HOST_PAYLOAD.json')
stage = Path(payload['stage'])
portable = 'Ottrin-0.1.0-windows-x64-portable.zip'
setup = 'Ottrin-0.1.0-windows-x64-setup.exe'
assert result['status'] == 'pass' and result['package'] == inputs
assert not result['identity']['administrator'] and result['identity']['session'] > 0
assert result['desktop_ready'] and result['owned_portable_extraction_removed']
assert result['owned_installation_removed'] and result['uninstall_exit'] == 0
assert all(result[key] for key in ('no_index', 'source_preserved', 'preferences_preserved'))
assert inputs['source_revision'] == payload['source_revision']
assert inputs['actor_sha256'] == digest(root / 'actor.ps1')
assert inputs['files'] == [{'name': 'portable.zip', 'sha256': payload['artifacts'][portable]},
                           {'name': 'current-setup.exe', 'sha256': payload['artifacts'][setup]}]
for name in (portable, setup):
    assert digest(stage / name) == payload['artifacts'][name]
assert result['binary_sha256'] == result['installed_binary_sha256'] == payload['payload_hashes']['bin/ottrin.exe']
assert result['remembered_folder_verified']
density = result['list_density']
assert set(density) == {'portable', 'installed'}
for mode in ('portable', 'installed'):
    compact, large = density[mode]
    assert [compact['scale'], large['scale']] == [0.9, 1.35]
    assert large['pitch'] - compact['pitch'] >= 8 and large['height'] > compact['height']
    assert compact['opened_expected_file'] and large['opened_expected_file']
preview = result['installed_preview_transition']
assert preview['files'] == ['preview-image.png', 'preview.pdf', 'preview-second.pdf']
assert preview['unchanged'] and preview['same_process'] and preview['normal_exit']
themes = result['installed_theme']
assert [case['theme'] for case in themes] == ['Dark', 'Light']
assert all(case['physical_cancel'] and case['preview_menu'] and case['normal_exit'] for case in themes)
for variant in themes:
    selector = variant['component_selector']
    assert selector['name'] == 'Theme component'
    assert selector['before'] == 'Theme defaults'
    assert selector['after'] == 'Buttons & menus'
    assert selector['typed_button_corners'] == 14
    assert selector['restore_verified']
cases = result['picker_cases']
assert [row['case'] for row in cases] == ['portable-grid', 'installed-hidden', 'portable-untouched']
assert len({row['process_id'] for row in cases}) == 3
assert result['grid_and_hidden_preserved']
assert all(row['normal_cancel'] and not row['response']['accepted'] and not row['response']['paths'] for row in cases)
cleanup = read(case / 'OWNER.json')['cleanup']
assert cleanup['domain_removed'] and cleanup['fixture_removed'] and cleanup['base_still_off']
assert len(cleanup['owned_volumes_removed']) == 2 and not cleanup['retained_pending_volume']
assert read(case / 'viewer/TEARDOWN.json')['profile_removed']
assert space['fixture_exit'] == 0 and space['compression_stage_removed']
for row in space['restored_by_hash']:
    assert digest(Path(row['path'])) == row['sha256']

archive = case / 'RESULTS.zip'
archive_hash = digest(archive)
assert archive_hash == read(case / 'ARCHIVE_TRANSFER.json')['sha256']
with zipfile.ZipFile(archive) as bundle:
    assert bundle.testzip() is None
    assert json.loads(bundle.read('RESULT.json').decode('utf-8-sig')) == result
    for name in preview['files']:
        screenshot = bundle.read(name + '.png')
        assert screenshot.startswith(b'\x89PNG\r\n\x1a\n')
        (case / (name + '.png')).write_bytes(screenshot)

retire = [archive, *case.glob('desktop-ready.png'), *case.glob('privacy-accepted-*.png')]
for path in retire:
    assert not path.is_symlink() and path.stat().st_nlink == 1
    assert subprocess.run(['fuser', str(path)], capture_output=True).returncode == 1
receipt = {
    'status': 'pass native installed theme component selector and picker regressions', 'input': inputs, 'artifact_hashes': payload['artifacts'], 'installed_uninstall_passed': True, 'binary_sha256': result['binary_sha256'],
    'cases': cases, 'list_density': density, 'installed_theme': themes, 'installed_preview_transition': preview, 'grid_and_hidden_preserved': True, 'no_index': True, 'preferences_and_fixture_preserved': True,
    'scope': 'Exact staged ZIP and NSIS in a disposable ordinary-user Windows VM. Both packages changed row geometry with List density and opened the exact clicked file. Three overlapping pickers preserved independent view settings. Installed Dark and Light exercised the named selector and Button corners edit, restore and Cancel. Installed Quick Inspector opened an image and both PDFs in one process, preserved the files and closed normally.',
    'cleanup': cleanup, 'compiler_outputs_restored': len(space['restored_by_hash']),
    'retired': [str(path.relative_to(project)) for path in retire],
    'export_sha256': archive_hash,
}
(root / 'ACCEPTANCE.json').write_text(json.dumps(receipt, indent=2) + '\n')
for path in retire:
    path.unlink()
