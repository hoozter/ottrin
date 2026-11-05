"""Accept exact fresh MSIX runtime scope only after native outcomes and teardown."""
from pathlib import Path
import argparse,hashlib,json,subprocess
parser=argparse.ArgumentParser();parser.add_argument('--case',choices=['fresh','update'],default='fresh');args=parser.parse_args()
is_update=args.case=='update'
r=Path.cwd();e=Path(__file__).resolve().parent;d=e/{'fresh':'menus25','update':'update24to25'}[args.case]
read=lambda p:json.loads(p.read_text())
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
v=read(d/'RUNTIME_RESULT.json');i=read(d/'RUNTIME_INPUT.json');o=read(d/'OWNER.json');a=v['actor'];login=v['login'];package=Path(read(e/'ASSEMBLY_ACCEPTANCE.json')['artifact'])
assert v['status']==a['status']==login['status']=='pass'
assert v['source_revision']==i['source_revision']==read(e/'ASSEMBLY_ACCEPTANCE.json')['source_revision']
assert i['version']==read(e/'ASSEMBLY_ACCEPTANCE.json')['version']=='0.1.0.25' and sha(package)==i['sha256']
if not is_update:assert v['deployment']['version']==i['version']
else:
 assert v['deployment']['version']==i['previous']['version']=='0.1.0.24'
 assert v['update']['before']==v['deployment']['full_name'] and v['update']['old_processes_removed']
 assert v['update']['family']==v['deployment']['family'] and v['update']['after']!=v['update']['before']
 assert v['pre_update_actor']['status']=='pass'
 assert all(a['store'][k]==v['pre_update_actor']['store'][k] for k in ['path','identity'])
 assert a['preferences_before_removal']==v['pre_update_actor']['preferences_before_removal']
assert sha(d/'RUNTIME_SOURCE.ps1')==i['runtime_script_sha256'] and sha(d/'NATIVE_CONTROLS_SOURCE.ps1')==i['native_controls_sha256']
assert a['main']['binary_sha256'].lower()==read(e/'PAYLOAD.json')['bin/ottrin.exe']
assert not a['administrator'] and not a['actual_process_elevated']
if not is_update:assert not a['daemon']['administrator'] and a['daemon']['package']==a['actual_process_package']
else:
 previous_daemon=v['pre_update_actor']['daemon'];assert not previous_daemon['administrator'] and previous_daemon['package']==v['deployment']['full_name']
assert a['actual_process_package']==(v['deployment']['full_name'] if not is_update else v['update']['after'])==login['observed_before_any_ottrin_cli']['package']
assert not login['observed_before_any_ottrin_cli']['administrator'] and not login['administrator']
assert a['normal_close_exit']==0 and a['gui_focus_and_capture']=='pass' and (a if not is_update else v['pre_update_actor'])['disable_reenable']=='pass'
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
assert folder['title']=='Open in Ottrin' and folder['initial_enabled']==is_update
assert [x['enabled'] for x in folder['actions']]==([False,True] if is_update else [True,False,True])
assert [x['com_state'] for x in folder['actions']]==([2,0] if is_update else [0,2,0])
assert len(folder['daemon_pids_before'])==1 and folder['daemon_pids_before']==folder['daemon_pids_after']
invocation=folder['invocation'];assert invocation['exit']==0 and invocation['ordinary'] and invocation['package']==a['actual_process_package']
assert invocation['visible_marker']=='menu-invocation-proof.txt' and invocation['pid']!=a['main']['pid']
assert invocation['selected_folder'].endswith('-- folder with spaces Ω𐐷')
assert all(x['complete_status_bounds']['width']>0 and x['complete_status_bounds']['height']>0 for x in folder['actions'])
assert folder['invocation_processes'], 'Actual OS argv observations required'
capture_review=read(d/'CAPTURE_REVIEW.json');assert capture_review['reviewed'] and capture_review['all_captures_match_exact_hashes']
assert capture_review['explorer_menu_visible'] and capture_review['invoked_main_visible']
for name,digest in capture_review['captures'].items():assert sha(d/'results'/name)==digest
contexts=folder['contexts'];assert [x['context'] for x in contexts]==['Background','Multiple','Drive']
for c,count in zip(contexts,[1,2,1]):
 assert c['status']=='pass actual Explorer context' and c['automatic_foreground']
 assert len(c['targets'])==len(c['processes'])==count and len({p['pid'] for p in c['processes']})==count
 assert len(c['daemon_before'])==1 and c['daemon_before']==c['daemon_after']
 for target in c['targets']:
  matches=[p for p in c['processes'] if p['path']==target['path']];assert len(matches)==1
  process=matches[0];assert process['closed'] and process['exit_code']==0 and process['marker']==target['marker']
  assert process['package']==a['actual_process_package'] and process['sid']==v['actor_sid']
assert v['com_unregistration']['status']=='pass' and not v['com_unregistration']['administrator'] and v['com_unregistration']['sid']==v['actor_sid']
assert v['com_unregistration']['activation_hresult']==-2147221164

result={'status':'pass exact MSIX25 native Explorer contexts, ordinary runtime/login and COM unregistration',
 'package':i,'source_revision':v['source_revision'],'runtime_result':str((d/'RUNTIME_RESULT.json').relative_to(r)),
 'folder_command':folder,'contexts':contexts,'startup_checkbox':proof,'real_login':login,'com_unregistration':v['com_unregistration'],
 'removal':{key:v[key] for key in ['package_removed','package_processes_removed','fixture_preserved','preferences_preserved','remaining_index_tasks']},
 'state_reader_native_smoke':smoke,'capture_review':capture_review,'cleanup':o['cleanup'],
 'limits':['Actual named physical folder background, two selected folders and OS drive on owned software desktop; no arbitrary virtual namespace acceptance',
 'Dark targeted originals only; no full theme/interface, physical graphics, Store, signing, owner or stable Windows delivery acceptance']}
if is_update:
 assert v['pre_update_actor']['previous_folder_command']['enabled'] and v['pre_update_actor']['previous_folder_command']['com_state']==0
 result['status']='pass exact MSIX24-to25 update, retained enabled command, native Explorer contexts and COM unregistration'
 result['update']=v['update'];result['previous_folder_command']=v['pre_update_actor']['previous_folder_command']
(e/{'fresh':'MSIX_CONTEXTS_ACCEPTANCE.json','update':'MSIX_ENABLED_UPDATE_ACCEPTANCE.json'}[args.case]).write_text(json.dumps(result,indent=2)+'\n')
print('Exact native scope accepted: '+args.case)
