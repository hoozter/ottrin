"""Accept the exact native service/login matrix only after original-image review."""
from pathlib import Path
import hashlib,json,subprocess
r=Path.cwd();e=Path(__file__).resolve().parent;f=e/'services'
read=lambda p:json.loads(p.read_text())
def sha(p):return hashlib.file_digest(p.open('rb'),'sha256').hexdigest()
v=read(f/'GUEST_RESULT.json');owner=read(f/'OWNER.json');source=read(e/'BUILD_SOURCE.json')['source_revision'];host=read(e/'HOST_PAYLOAD.json')
assert v['status']=='pass independent native Services workflow and preserved uninstall'
assert v['package']['source_revision']==source and v['package']['actor_sha256']==sha(f/'actor.ps1')==sha(e/'services-actor.ps1')
assert v['package']['files'][0]['sha256']==host['artifacts']['Ottrin-0.1.0-windows-x64-setup.exe']
assert v['source_preserved'] and v['preferences_preserved'] and v['remaining_owned_installed_processes']==0
matrix=v['service_matrix'];assert matrix['status']=='pass native service and independent login state scope'
assert matrix['source_and_preferences_preserved'] and matrix['saved_index_items']==1
expected=[(False,False,True),(True,True,True),(True,False,True),(True,True,True),(True,True,True),(True,True,False),(True,False,False),(True,True,False),(True,True,True),(True,False,True),(True,False,True),(True,False,False),(True,False,True),(True,True,True),(True,False,True),(False,False,False)]
assert len(matrix['states'])==len(expected)
for row,state in zip(matrix['states'],expected):
 assert (row['installed'],row['login_enabled'],row['running'])==state
 assert row['daemon_processes']==int(row['running'])
 if row['installed']:assert row['task_enabled']==row['login_enabled'] and row['task_name'].startswith('Ottrin Index S-')
 else:assert row['task_name'] is None and row['task_instances']==0
 if not row['running']:assert row['task_instances']==0
for index in [1,4,8]:assert matrix['states'][index]['task_instances']>0
checks=matrix['login_toggle_checks'];assert len(checks)==6
for check,state in zip(checks,[(False,True),(True,True),(False,False),(True,False),(True,True),(False,True)]):
 assert (check['enabled'],check['running'])==state and check['permissions_preserved']
 assert len(check['daemon_pids_preserved'])==int(check['running'])
assert checks[4]['task_instances_preserved']==checks[5]['task_instances_preserved']==0
review=read(e/'WINDOWS_SERVICES_ORIGINAL_REVIEW.json');assert review['directly_reviewed_originals']==7
names={'services-session-running.png','services-installed-running.png','services-installed-stopped.png','services-disabled-running.png','services-disabled-stopped.png','services-enabled-session-running.png','services-uninstalled.png'}
assert set(review['captures'])==names
for name,digest in review['captures'].items():assert sha(f/'results'/name)==digest
assert owner['cleanup']['domain_removed'] and owner['cleanup']['fixture_removed'] and len(owner['cleanup']['owned_volumes_removed'])==2
current=subprocess.run(['virsh','-c','qemu:///system','domuuid',owner['domain']],capture_output=True,text=True);assert current.returncode or current.stdout.strip()!=owner['uuid']
assert not Path(owner['private_console']['home']).parent.exists()
receipt={'status':'pass scoped native Windows service/login control lifecycle','source_revision':source,'installer_sha256':v['package']['files'][0]['sha256'],'states':16,'login_toggle_checks':6,'full_window_captures':7,'captures':review['captures'],'cleanup':owner['cleanup'],'checks':['Actual ordinary-user NSIS install and supported uninstall','Initial setup, update/reinstall, stop, restart, disabled registration repair, external startup choice and removal','Six native checkbox actions preserve current daemon PID set, task instances and permissions','Stopped and running intent remain independent of future login enablement; managed setup/repair/restart has a scheduled instance','Source, preferences and saved index preserved'],'limits':['Owned software-renderer Dark144DPI OpenGL VM; core9case settings and other renderer separate','No reboot/login activation, MSIX startup, Linux service/polkit, physical or final human acceptance']}
(e/'WINDOWS_SERVICES_ACCEPTANCE.json').write_text(json.dumps(receipt,indent=2)+'\n');print(receipt['status'])
