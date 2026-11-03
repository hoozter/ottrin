"""Accept only exact native update/login/removal facts and completed fixture teardown."""
from pathlib import Path
import argparse,hashlib,json,zipfile
r=Path.cwd();root=Path(__file__).resolve().parent
parser=argparse.ArgumentParser();parser.add_argument('--case',type=Path,required=True);e=parser.parse_args().case.resolve()
assert e.parent==root and e.name in {'upgrade-enabled-native-focus','upgrade-disabled-console'}
def read(name):return json.loads((e/name).read_text())
expected=read('RUNTIME_INPUT.json');result=read('RUNTIME_RESULT.json');owner=read('OWNER.json');viewer=read('viewer/TEARDOWN.json')
assert expected['source_revision']==result['source_revision']
accepted=json.loads((root/'ASSEMBLY_ACCEPTANCE.json').read_text())
assert expected['sha256']==accepted['artifact_sha256'] and expected['version']==accepted['version']=='0.1.0.6'
assert hashlib.sha256(Path(accepted['artifact']).read_bytes()).hexdigest()==expected['sha256']
previous=json.loads((root/'PREVIOUS_PACKAGE.json').read_text())
assert expected['previous']['version']=='0.1.0.5' and expected['previous']['sha256']==previous['sha256']
assert hashlib.sha256(Path(previous['retained_input']).read_bytes()).hexdigest()==previous['sha256']
assert result['status']=='pass'
for key in ['package_removed','package_processes_removed','preferences_preserved','fixture_preserved']:assert result[key] is True,key
assert result['remaining_index_tasks']==[]
assert result['update']['old_processes_removed'] is True
assert result['update']['startup_enabled'] is expected['startup_enabled']
assert result['update']['previous_daemon_count']==int(expected['startup_enabled'])
assert result['update']['before'].startswith('Ottrin.Development_0.1.0.5_')
assert result['update']['after'].startswith('Ottrin.Development_0.1.0.6_')
actors=[result['pre_update_actor'],result['updated_actor']]
manifest=json.loads((root/'PAYLOAD.json').read_text())
with zipfile.ZipFile(previous['retained_input']) as z:old_binary=hashlib.sha256(z.read('bin/ottrin.exe')).hexdigest()
for actor,binary,identity in zip(actors,[old_binary,manifest['bin/ottrin.exe']],[result['update']['before'],result['update']['after']]):
 assert actor['status']=='pass' and actor['gui_focus_and_capture']=='pass'
 assert actor['administrator'] is False and actor['actual_process_elevated'] is False
 assert actor['actual_process_package']==identity and actor['normal_close_exit']==0
 assert actor['main']['binary_sha256'].lower()==binary
 assert actor['cli_exits'] and all(v==0 for v in actor['cli_exits'].values())
 assert actor['input_desktop']=='Default'
assert actors[0]['disable_reenable']=='pass'
assert actors[0]['preferences_before_removal']==actors[1]['preferences_before_removal']
login=result['login'];assert login['status']=='pass' and login['administrator'] is False and login['preferences_preserved'] is True
assert all(login['store'][k]==actors[0]['store'][k]==actors[1]['store'][k] for k in ['path','identity'])
assert login['boot_ticks']!=result['before_login_boot_ticks']
for n in [1,2]:
 reboot=read(f'REBOOT_{n}.json');assert reboot['status']=='normal reboot verified' and reboot['before_boot_ticks']!=reboot['after_boot_ticks']
assert login['boot_ticks']==read('REBOOT_2.json')['after_boot_ticks']
observation=login['observed_before_any_ottrin_cli']
if expected['startup_enabled']:
 assert observation['pid']>0 and observation['session']>0 and observation['administrator'] is False
 assert observation['package']==result['update']['after'] and login['index_ready']=='ready'
else:
 assert observation['daemon_count']==0 and observation['startup_enabled'] is False and observation['observation_seconds']>=180
 assert login['index_state']=='stopped as configured'
keys=actors[1]['updated_menu_keys'];assert keys['status']=='pass'
for key in ['normal_space_open_close','inspector_menu_escape_keeps_window_open','source_and_directory_unchanged','main_menu_space_keeps_inspector_closed']:assert keys[key] is True
cleanup=owner['cleanup'];assert cleanup['domain_removed'] and cleanup['fixture_removed'] and cleanup['base_still_off'] and len(cleanup['owned_volumes_removed'])==2
assert viewer['profile_removed'] and not Path(viewer['profile']).exists()
archive=zipfile.ZipFile(e/'RESULTS.zip') if (e/'RESULTS.zip').exists() else None
try:
 original=archive.read('RESULT.json') if archive else (e/'results/RESULT.json').read_bytes()
 assert json.loads(original.decode('utf-8-sig'))==result
 for name in ['main.png','updated-main.png','updated-main-menu-before-space.png','updated-main-menu-after-space.png','updated-inspector-menu.png','updated-inspector-before-normal-space.png']:
  data=archive.read(name) if archive else (e/'results'/name).read_bytes();assert data.startswith(b'\x89PNG\r\n\x1a\n')
finally:
 if archive:archive.close()
receipt={'status':'pass within actual native update/ordinary GUI/real reboot/login/removal scope','source_revision':result['source_revision'],'package_input':expected,'native_receipt':'RUNTIME_RESULT.json','updated_menu_keys':keys,'preferences_and_index_file_identity_preserved':True,'actual_login':observation,'owned_vm_cleanup':cleanup,'private_console_cleanup_verified':True,'native_visibility_receipt':'Captured for separate hash-bound rendered review; not accepted by this lifecycle assessor','scope_limit':'Owned Windows software SPICE, exact package identity and ordinary runtime token; unsigned development deployment uses the explicit administrative fixture. Not Store signing/certification, physical graphics, current NSIS/MSI acceptance or whole-Windows-roadmap completion.'}
(e/'ACCEPTANCE.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(receipt))
