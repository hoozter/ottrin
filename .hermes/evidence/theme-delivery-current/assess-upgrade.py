"""Accept exact16-to17 MSIX updates for enabled and disabled startup choices."""
from pathlib import Path
import hashlib,json,subprocess
r=Path.cwd();e=Path(__file__).resolve().parent
read=lambda p:json.loads(p.read_text())
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
previous=read(e/'PREVIOUS_PACKAGE.json');assert sha(Path(previous['retained_input']))==previous['sha256']
current=read(e/'ASSEMBLY_ACCEPTANCE.json');assert sha(Path(current['artifact']))==current['artifact_sha256']
results=[]
for case,enabled in [('runtime17-upgrade-space',True),('runtime17-upgrade-disabled',False)]:
 d=e/case;v=read(d/'RUNTIME_RESULT.json');i=read(d/'RUNTIME_INPUT.json');o=read(d/'OWNER.json');a=v['updated_actor'];old=v['pre_update_actor'];login=v['login'];update=v['update']
 assert v['status']==old['status']==a['status']==login['status']=='pass'
 assert i['source_revision']==current['source_revision']=='45830098b6d58eeb8a08252f8d8b9c6fe35d9eef' and i['sha256']==current['artifact_sha256'] and i['version']=='0.1.0.17'
 assert i['previous']=={k:previous[k] for k in ['sha256','version','source_revision']} and i['startup_enabled'] is enabled
 assert update['before'].startswith('Ottrin.Development_0.1.0.16_') and update['after'].startswith('Ottrin.Development_0.1.0.17_') and update['old_processes_removed'] and update['startup_enabled'] is enabled
 assert old['actual_process_package']==update['before'] and a['actual_process_package']==update['after'] and not a['actual_process_elevated'] and not a['administrator']
 assert a['main']['binary_sha256'].lower()==read(e/'PAYLOAD.json')['bin/ottrin.exe']
 assert a['gui_focus_and_capture']=='pass' and a['normal_close_exit']==0 and a['preferences_before_removal']==old['preferences_before_removal']
 assert a['store']['identity']==old['store']['identity']==login['store']['identity'] and a['store']['path']==old['store']['path']==login['store']['path']
 assert login['boot_ticks']>v['before_login_boot_ticks'] and login['preferences_preserved'] and not login['administrator']
 observed=login['observed_before_any_ottrin_cli']
 if enabled:assert observed['package']==update['after'] and not observed['administrator'] and login['index_ready']=='ready'
 else:assert observed['daemon_count']==0 and observed['startup_enabled'] is False and observed['observation_seconds']>=180 and login['index_state']=='stopped as configured'
 assert all(code==0 for code in a['cli_exits'].values()) and all(code==0 for code in login['cli_exits'].values())
 assert all(v[k] for k in ['package_removed','package_processes_removed','preferences_preserved','fixture_preserved']) and not v['remaining_index_tasks']
 assert not read(d/'results/CRASH_EVENTS.json')['events']
 assert sha(d/'RUNTIME_SOURCE.ps1')==i['runtime_script_sha256']
 assert o['cleanup']['domain_removed'] and o['cleanup']['fixture_removed'] and len(o['cleanup']['owned_volumes_removed'])==2 and not Path(o['private_console']['home']).parent.exists()
 identity=subprocess.run(['virsh','-c','qemu:///system','domuuid',o['domain']],capture_output=True,text=True);assert identity.returncode or identity.stdout.strip()!=o['uuid']
 results.append({'case':case,'startup_enabled':enabled,'runtime_input':i,'update':update,'real_login':login,'store_identity_preserved':True,'preferences_preserved':True,'normal_close_exit':a['normal_close_exit'],'cleanup':o['cleanup'],'volume_allocations':o['owned_volume_allocations_before_removal']})
(e/'MSIX_UPGRADE_ACCEPTANCE.json').write_text(json.dumps({'status':'pass exact same-identity MSIX16-to17 updates with enabled and disabled startup, real reboot/login, preserved index/preferences and removal','source_revision':current['source_revision'],'artifact_sha256':current['artifact_sha256'],'previous':previous,'cases':results,'limits':['Unsigned development identity in owned software desktops; no Store/signing or physical acceptance','Broader theme/role audit remains open']},indent=2)+'\n');print('Both exact MSIX16-to17 startup-choice upgrade cases accepted')
