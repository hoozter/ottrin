"""Accept exact fresh MSIX runtime scope only after native outcomes and teardown."""
from pathlib import Path
import argparse,hashlib,json,subprocess
parser=argparse.ArgumentParser();parser.add_argument('--case',choices=['fresh','update','menus'],default='fresh');args=parser.parse_args()
is_update=args.case=='update'
r=Path.cwd();e=Path(__file__).resolve().parent;d=e/{'fresh':'runtime24','update':'update23to24','menus':'menus24'}[args.case]
read=lambda p:json.loads(p.read_text())
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
v=read(d/'RUNTIME_RESULT.json');i=read(d/'RUNTIME_INPUT.json');o=read(d/'OWNER.json');a=v['actor'];login=v['login'];package=Path(read(e/'ASSEMBLY_ACCEPTANCE.json')['artifact'])
assert v['status']==a['status']==login['status']=='pass'
assert v['source_revision']==i['source_revision']==read(e/'ASSEMBLY_ACCEPTANCE.json')['source_revision']
assert i['version']=='0.1.0.24' and sha(package)==i['sha256']
if not is_update:assert v['deployment']['version']==i['version']
else:
 assert v['deployment']['version']==i['previous']['version']=='0.1.0.23'
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
assert folder['title']=='Open in Ottrin' and [x['enabled'] for x in folder['actions']]==[True,False,True]
assert [x['com_state'] for x in folder['actions']]==[0,2,0]
assert len(folder['daemon_pids_before'])==1 and folder['daemon_pids_before']==folder['daemon_pids_after']
invocation=folder['invocation'];assert invocation['exit']==0 and invocation['ordinary'] and invocation['package']==a['actual_process_package']
assert invocation['visible_marker']=='menu-invocation-proof.txt' and invocation['pid']!=a['main']['pid']
assert invocation['selected_folder'].endswith('-- folder with spaces Ω𐐷')
assert all(x['complete_status_bounds']['width']>0 and x['complete_status_bounds']['height']>0 for x in folder['actions'])
assert folder['invocation_processes'], 'Actual OS argv observations required'
if is_update:
 assert folder['explorer']['status']=='pass actual Explorer menu invocation' and 'Open in Ottrin' in folder['explorer']['menu_items']
 explorer=read(d/'results/EXPLORER_INVOKED_PROCESS.json')
 assert explorer['closed'] and explorer['exit_code']==0 and explorer['foreground_before_qa_focus']
 assert explorer['package']==a['actual_process_package'] and explorer['sid']==v['actor_sid'] and explorer['ordinary']
 assert v['com_unregistration']['status']=='pass' and not v['com_unregistration']['administrator']
 assert v['com_unregistration']['sid']==v['actor_sid'] and v['com_unregistration']['activation_hresult']==-2147221164
capture_review=read(d/'CAPTURE_REVIEW.json');assert capture_review['reviewed'] and capture_review['all_captures_match_exact_hashes']
assert capture_review['settings_visible'] if args.case=='fresh' else capture_review['explorer_menu_visible'] and capture_review['invoked_main_visible']
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
if args.case=='menus':
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
 result['status']='pass actual Explorer background, two-folder selection, drive, ordinary runtime/login and COM unregistration'
 result['contexts']=contexts;result['com_unregistration']=v['com_unregistration']
 result['limits']=['Exact24 selected physical folders, background and OS drive on owned software desktop; no virtual namespace or later enabled-preference upgrade acceptance','No whole-interface, physical graphics, Store, signing, owner or stable Windows delivery acceptance']
if is_update:
 result['status']='pass exact MSIX23-to24 ordinary upgrade, actual Explorer folder menu, real login and COM unregistration'
 result['update']=v['update'];result['com_unregistration']=v['com_unregistration'];result['explorer_invoked_process']=explorer
 result['limits']=['Actual selected-folder Explorer menu invocation only; background/drive/multiselection remain separate','Dark owned software desktop; no full UI, physical GPU, Store, signing or owner acceptance','Current Windows stable-path publication requires separate NSIS UI package gates']
(e/{'fresh':'MSIX_RUNTIME_ACCEPTANCE.json','update':'MSIX_UPDATE_ACCEPTANCE.json','menus':'MSIX_CONTEXTS_ACCEPTANCE.json'}[args.case]).write_text(json.dumps(result,indent=2)+'\n')
print('Exact native scope accepted: '+args.case)
