from pathlib import Path
import hashlib
import json
import subprocess
import sys
import zipfile

root = Path(__file__).resolve().parent
case = root / sys.argv[1]
assert case.parent == root and case.name in ('runtime40-fresh', 'runtime40-upgrade', 'runtime40-disabled')

def read(path):
    return json.loads(path.read_text())

result = read(case / 'RUNTIME_RESULT.json')
owner = read(case / 'OWNER.json')
inputs = read(case / 'RUNTIME_INPUT.json')
assembly = read(root / 'ASSEMBLY_ACCEPTANCE.json')
assert result['status'] == 'pass' and result['actor']['status'] == 'pass'
assert result['source_revision'] == inputs['source_revision'] == assembly['source_revision']
assert inputs['sha256'] == assembly['artifact_sha256']
assert result['deployment']['version'] == ('0.1.0.40' if case.name.endswith('fresh') else '0.1.0.39')
assert not result['actor']['administrator'] and not result['actor']['actual_process_elevated']
assert result['actor']['administrator_copy_availability']['notice_visible']
assert result['actor']['administrator_copy_availability']['screenshot'] == 'msix-administrator-copy-availability.png'
assert result['actor']['administrator_copy_availability']['app_helper_ready_visible']
build = read(root / 'HOST_PAYLOAD.json')
assert result['actor']['administrator_copy_availability']['helper_sha256'] == build['payload_hashes']['bin/ottrin-priv-helper.exe']
assert result['login']['status'] == 'pass'
assert result['login']['boot_ticks'] != result['before_login_boot_ticks']
assert result['com_unregistration']['status'] == 'pass'
assert result['actor']['store']['identity'] == result['login']['store']['identity']
assert all(result[k] for k in ('fixture_preserved', 'preferences_preserved', 'package_removed', 'package_processes_removed'))
assert not result['remaining_index_tasks']
assert result['actor']['default_folder_availability']['host_registry_unchanged']
contexts = result['actor']['folder_command']['contexts']
assert {row['context'] for row in contexts} == {'Background', 'Multiple', 'Drive'}
assert all(row['status'] == 'pass actual Explorer context' for row in contexts)
if case.name.endswith('fresh'):
    assert result['login']['observed_before_any_ottrin_cli']['package'] == result['deployment']['full_name']
else:
    assert result['update']['old_processes_removed']
    assert result['update']['startup_enabled'] == inputs['startup_enabled']
    assert '0.1.0.40' in result['update']['after']
    assert result['pre_update_actor']['store']['identity'] == result['actor']['store']['identity']
cleanup = owner['cleanup']
assert cleanup['domain_removed'] and cleanup['fixture_removed'] and cleanup['base_still_off']
assert len(cleanup['owned_volumes_removed']) == 2 and not cleanup['retained_pending_volume']
assert read(case / 'viewer/TEARDOWN.json')['profile_removed']
archive = case / 'RESULTS.zip'
receipt_path = case / 'ACCEPTANCE.json'
if not archive.exists():
    receipt = read(receipt_path)
    assert receipt['status'] == 'pass native MSIX deployment, actual login and removal'
    assert receipt['input'] == inputs and receipt['actual_login'] == result['login']
    assert receipt['cleanup'] == cleanup
    assert receipt['verified_export_sha256'] == read(case / 'ARCHIVE_TRANSFER.json')['sha256']
    assert all(not Path(path).exists() for path in receipt['disposable_outputs_retired'])
    assert (case / 'msix-default-folder-availability.png').is_file()
    raise SystemExit(0)
with archive.open('rb') as stream:
    digest = hashlib.file_digest(stream, 'sha256').hexdigest()
assert digest == read(case / 'ARCHIVE_TRANSFER.json')['sha256']
with zipfile.ZipFile(archive) as bundle:
    assert json.loads(bundle.read('RESULT.json').decode('utf-8-sig')) == result
    assert bundle.testzip() is None
    screenshot = 'msix-default-folder-availability.png'
    (case / screenshot).write_bytes(bundle.read(screenshot))
    notice = result['actor']['administrator_copy_availability']['screenshot']
    assert bundle.getinfo(notice).file_size > 0
    (case / notice).write_bytes(bundle.read(notice))
retire = [archive, *case.glob('desktop-ready.png'), *case.glob('privacy-accepted-*.png')]
for path in retire:
    assert not path.is_symlink() and path.stat().st_nlink == 1
    assert subprocess.run(['fuser', str(path)], capture_output=True).returncode == 1
receipt = {
    'status': 'pass native MSIX deployment, actual login and removal',
    'case': case.name,
    'input': inputs,
    'ordinary_user': True,
    'administrator_copy_availability': result['actor']['administrator_copy_availability'],
    'actual_login': result['login'],
    'explorer_contexts': [row['context'] for row in contexts],
    'package_process_task_and_com_removal': True,
    'preferences_and_fixture_preserved': True,
    'update': result.get('update'),
    'cleanup': cleanup,
    'verified_export_sha256': digest,
    'disposable_outputs_retired': [str(path) for path in retire],
}
for path in retire:
    path.unlink()
receipt_path.write_text(json.dumps(receipt, indent=2) + '\n')
