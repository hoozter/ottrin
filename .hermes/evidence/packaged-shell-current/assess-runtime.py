"""Accept exact fresh MSIX runtime scope only after native outcomes and teardown."""
from pathlib import Path
import hashlib,json,subprocess
r=Path.cwd();e=Path(__file__).resolve().parent;d=e/'runtime24'
read=lambda p:json.loads(p.read_text())
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
v=read(d/'RUNTIME_RESULT.json');i=read(d/'RUNTIME_INPUT.json');o=read(d/'OWNER.json');a=v['actor'];login=v['login'];package=Path(read(e/'ASSEMBLY_ACCEPTANCE.json')['artifact'])
assert v['status']==a['status']==login['status']=='pass'
assert v['source_revision']==i['source_revision']==read(e/'ASSEMBLY_ACCEPTANCE.json')['source_revision']
assert i['version']==v['deployment']['version']=='0.1.0.24' and sha(package)==i['sha256']
assert sha(d/'RUNTIME_SOURCE.ps1')==i['runtime_script_sha256'] and sha(d/'NATIVE_CONTROLS_SOURCE.ps1')==i['native_controls_sha256']
assert a['main']['binary_sha256'].lower()==read(e/'PAYLOAD.json')['bin/ottrin.exe']
assert not a['administrator'] and not a['actual_process_elevated'] and not a['daemon']['administrator']
assert a['actual_process_package']==a['daemon']['package']==v['deployment']['full_name']==login['observed_before_any_ottrin_cli']['package']
assert not login['observed_before_any_ottrin_cli']['administrator'] and not login['administrator']
assert a['normal_close_exit']==0 and a['gui_focus_and_capture']=='pass' and a['disable_reenable']=='pass'
assert a['index_ready']==login['index_ready']=='ready' and all(code==0 for code in a['cli_exits'].values()) and all(code==0 for code in login['cli_exits'].values())
assert login['boot_ticks']>v['before_login_boot_ticks'] and login['store']['identity']==a['store']['identity'] and login['preferences_preserved']
assert v['package_removed'] and v['package_processes_removed'] and v['fixture_preserved'] and v['preferences_preserved'] and not v['remaining_index_tasks']
assert not read(d/'results/CRASH_EVENTS.json')['events']
proof=a['msix_startup_checkbox'];assert proof['status']=='pass actual installed-package checkbox' and [x['enabled'] for x in proof['actions']]==[False,True]
for x in proof['actions']:assert len(x['daemon_pids_before'])==1 and x['daemon_pids_before']==x['daemon_pids_after']
assert o['cleanup']['domain_removed'] and o['cleanup']['fixture_removed'] and len(o['cleanup']['owned_volumes_removed'])==2
assert not Path(o['private_console']['home']).parent.exists()
identity=subprocess.run(['virsh','-c','qemu:///system','domuuid',o['domain']],capture_output=True,text=True);assert identity.returncode or identity.stdout.strip()!=o['uuid']
smoke=read(d/'results/STATE_READER_SMOKE.json');assert smoke['private_fixture_removed'] and smoke['status'].startswith('pass native sharing retry')
folder=a['folder_command']
assert folder['status'].startswith('pass native packaged COM activation') and folder['host_package_identity_code']==15700
assert folder['title']=='Open in Ottrin' and [x['enabled'] for x in folder['actions']]==[True,False,True]
assert [x['com_state'] for x in folder['actions']]==[0,2,0]
assert len(folder['daemon_pids_before'])==1 and folder['daemon_pids_before']==folder['daemon_pids_after']
invocation=folder['invocation'];assert invocation['exit']==0 and invocation['ordinary'] and invocation['package']==a['actual_process_package']
assert invocation['visible_marker']=='menu-invocation-proof.txt' and invocation['pid']!=a['main']['pid']
assert invocation['selected_folder'].endswith('-- folder with spaces Ω𐐷')
assert all(x['complete_status_bounds']['width']>0 and x['complete_status_bounds']['height']>0 for x in folder['actions'])
assert folder['invocation_processes'], 'Actual OS argv observations required'
capture_review=read(d/'CAPTURE_REVIEW.json');assert capture_review['reviewed'] and capture_review['settings_visible'] and capture_review['all_captures_match_exact_hashes']
for name,digest in capture_review['captures'].items():assert sha(d/'results'/name)==digest
result={'status':'pass exact MSIX24 ordinary runtime, direct packaged COM folder invocation, real reboot/login and removal preservation',
 'package':i,'source_revision':v['source_revision'],'runtime_result':str((d/'RUNTIME_RESULT.json').relative_to(r)),
 'folder_command':folder,'startup_checkbox':proof,'real_login':login,
 'removal':{key:v[key] for key in ['package_removed','package_processes_removed','fixture_preserved','preferences_preserved','remaining_index_tasks']},
 'state_reader_native_smoke':smoke,'capture_review':capture_review,'cleanup':o['cleanup'],
 'limits':['Direct native COM API invocation, not an actual Explorer menu click or automatic foreground proof',
 'Real Explorer foreground/background/drive/multiselection, package upgrade and ordinary-user COM unregistration remain separate',
 'Dark software-desktop targeted captures only; no Light, whole-interface, physical GPU, Store, signing or owner acceptance',
 'Stable-path Windows publication requires its own remaining package gates; this is an investigation candidate']}
(e/'MSIX_RUNTIME_ACCEPTANCE.json').write_text(json.dumps(result,indent=2)+'\n')
print('Exact fresh MSIX24 direct COM/runtime/login/removal scope accepted; wider Explorer/update gates remain')
