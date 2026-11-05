"""Accept exact fresh MSIX runtime scope only after native outcomes and teardown."""
from pathlib import Path
import hashlib,json,subprocess
r=Path.cwd();e=Path(__file__).resolve().parent;d=e/'runtime21'
read=lambda p:json.loads(p.read_text())
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
v=read(d/'RUNTIME_RESULT.json');i=read(d/'RUNTIME_INPUT.json');o=read(d/'OWNER.json');a=v['actor'];login=v['login'];package=Path(read(e/'ASSEMBLY_ACCEPTANCE.json')['artifact'])
assert v['status']==a['status']==login['status']=='pass'
assert v['source_revision']==i['source_revision']=='781444efd5ed4f67c6848127935366356be5b6d1'
assert i['version']==v['deployment']['version']=='0.1.0.21' and sha(package)==i['sha256']
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
capture_review=read(d/'CAPTURE_REVIEW.json');assert capture_review['reviewed'] and capture_review['settings_visible'] and capture_review['all_captures_match_exact_hashes']
for name,digest in capture_review['captures'].items():assert sha(d/'results'/name)==digest
result={'capture_review':capture_review,'status':'pass exact MSIX21 functional runtime, real reboot/login and removal; visual scope requires capture review','package':i,'source_revision':v['source_revision'],'runtime_result':str((d/'RUNTIME_RESULT.json').relative_to(r)),'startup_checkbox':proof,'real_login':login,'removal':{key:v[key] for key in ['package_removed','package_processes_removed','fixture_preserved','preferences_preserved','remaining_index_tasks']},'state_reader_native_smoke':smoke,'original_captures_directly_reviewed':{str((d/'results'/name).relative_to(r)):sha(d/'results'/name) for name in ['main.png','msix-startup-disabled.png','msix-startup-enabled.png']},'fixture_recovery':read(d/'AUTOLOGON_RECOVERY.json') if (d/'AUTOLOGON_RECOVERY.json').exists() else None,'cleanup':o['cleanup'],'limits':['Fresh MSIX21 only; no MSIX20-to-21 upgrade acceptance','Unsigned development deployment in disposable software desktop; no signing, Store or physical hardware acceptance','Windows native preview pointer check and full theme audit remain open','Stable-path delivery has its own authoritative Windows candidate manifest; this receipt proves only the stated native runtime scope']}
(e/'MSIX_RUNTIME_ACCEPTANCE.json').write_text(json.dumps(result,indent=2)+'\n');print('Exact fresh MSIX21 runtime/login/removal accepted; no upgrade or whole-roadmap claim')
