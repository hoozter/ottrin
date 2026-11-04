"""Bind reviewed 150% captures and real-reboot count observations to this cohort."""
from pathlib import Path
import hashlib,json,subprocess
R=Path.cwd();E=Path(__file__).resolve().parent
read=lambda p:json.loads(p.read_text())
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def teardown(n):
 owner=read(n/'ubuntu/VM_OWNER.json');cleanup=read(n/'ubuntu/VM_CLEANUP.json')
 assert cleanup['domain_removed'] and len(cleanup['volumes_removed'])==3
 assert subprocess.run(['virsh','-c','qemu:///system','domuuid',owner['domain']],capture_output=True).returncode!=0
 for name in cleanup['volumes_removed']:assert subprocess.run(['virsh','-c','qemu:///system','vol-path',name,'--pool','default'],capture_output=True).returncode!=0
 assert not any((n/'ubuntu').glob('.native-vm-*'))
 assert sha(n/'actor.py')==owner['actor_sha256']
 assert sha(Path(owner['cleanup_owner']))==owner['controller_sha256']
 return cleanup
build=read(E/'BUILD_RESULT.json');content=read(E/'PACKAGE_CONTENT_ACCEPTANCE.json')
n=E/'scale150';out=n/'ubuntu/results';actor=read(out/'RESULT.json');worker=read(out/'WORKER_RESULT.json');review=read(n/'CAPTURE_REVIEW.json')
assert actor['status']==worker['status']=='pass' and actor['package_source']==build['source_revision']
assert actor['appimage_input']['sha256']==build['artifacts']['Ottrin-0.1.0-x86_64.AppImage']
assert worker['display']['size']==[1920,1200] and worker['display']['winit_scale_factor']==1.5
assert worker['physical_widths']==[1200]*4 and worker['picker_physical_widths']==[1410]*2
assert [(x['theme'],x['scale']) for x in worker['grid_cases']]==[('Dark',1.0),('Light',1.0),('Dark',1.85),('Light',1.85)]
assert len(worker['picker_cases'])==2 and all(x['cancelled'] for x in worker['picker_cases'])
assert all(x['gui_sha256']==content['runtime_sha256']['application'] for x in worker['grid_cases']+worker['picker_cases'])
names=[x['capture'] for x in worker['grid_cases']+worker['picker_cases']]
assert review['scope']=='Original complete-window captures directly reviewed'
for name in names:assert review['captures'][name]==sha(out/name)
archive=n/'ubuntu/VM_RESULTS.tar.gz';transport={'bytes':archive.stat().st_size,'sha256':sha(archive)};archive.unlink()
report={'status':'pass scoped actual AppImage150% browser/picker visual supplement','source_revision':build['source_revision'],'artifact_sha256':build['artifacts']['Ottrin-0.1.0-x86_64.AppImage'],'application_sha256':content['runtime_sha256']['application'],'display':worker['display'],'physical_widths':worker['physical_widths'],'picker_physical_widths':worker['picker_physical_widths'],'grid_cases':worker['grid_cases'],'picker_cases':worker['picker_cases'],'original_captures_directly_reviewed':{str((out/name).relative_to(R)):sha(out/name) for name in names},'notes':review['notes'],'cleanup':teardown(n),'consumed_transport_removed':transport,'limits':['Controlled software-X11 backend scaling inside owned guest; no physical graphics/Wayland/Windows or full theme audit.','Caption text is enlarged with185% Grid density; custom-theme font-setting acceptance is not inferred.']}
(E/'SCALE150_ACCEPTANCE.json').write_text(json.dumps(report,indent=2)+'\n')
n=E/'warm-counter';out=n/'ubuntu/results';vm=read(n/'ubuntu/VM_RESULT.json');phases=[read(out/f'PHASE{i}.json') for i in range(3)];review=read(n/'CAPTURE_REVIEW.json')
assert vm['status']=='pass within actor scope' and vm['source']==build['source_revision']
assert len(vm['boot_ids'])==len(set(vm['boot_ids']))==3
for i,p in enumerate(phases):
 assert p['status']=='pass' and p['phase']==i and p['boot_id']==vm['boot_ids'][i]
 assert p['gui_attempts']==1 and p['source_and_preferences_preserved']
 assert p['database_inode']==phases[0]['database_inode']
 assert p['after_gui_unit']['MainPID']==p['after_normal_gui_close_unit']['MainPID']
 assert read(out/f'ROOT_PHASE{i}.json')['daemon_sha256']==content['runtime_sha256']['index_daemon']
assert [p['startup_enabled'] for p in phases]==[True,False,False]
assert phases[1]['before_gui_auto_started_unit']['MainPID']==phases[1]['after_gui_unit']['MainPID']
assert phases[2]['disabled_before_gui_no_daemon']
samples=phases[2]['no_input_startup_samples'];assert len(samples)==4 and samples[-1]['seconds_after_window']>=30
assert all(x['item_labels']==['label | 1000 items'] for x in samples)
names=['main-phase0.png','main-phase1.png','main-phase2.png','disabled-login-running-index.png']+[f'warm-disabled-{i}.png' for i in [0,2,10,30]]
assert review['scope']=='Original complete-window captures directly reviewed'
for name in names:assert review['captures'][name]==sha(out/name)
assert not (n/'ubuntu/VM_RESULTS.tar.gz').exists()
report={'status':'pass scoped native DEB startup/lifetime and observations; initial-frame footer defect retained','source_revision':build['source_revision'],'deb_sha256':build['artifacts']['ottrin_0.1.0_amd64.deb'],'boot_ids':vm['boot_ids'],'startup_enabled_after_each_phase':[p['startup_enabled'] for p in phases],'gui_attempts':[p['gui_attempts'] for p in phases],'database_inode_preserved':True,'normal_gui_close_keeps_daemon':True,'disabled_warm_index_samples':samples,'original_captures_directly_reviewed':{str((out/name).relative_to(R)):sha(out/name) for name in names},'notes':review['notes'],'cleanup':teardown(n),'consumed_transports':vm['consumed_result_transports'],'limits':['Initial phase0 screenshot shows0 items beside populated rows. Correct disabled-warm observations do not erase that finding or prove a code fix.','No physical desktop, Wayland, Windows or sustained resource claim.']}
(E/'WARM_COUNTER_OBSERVATION.json').write_text(json.dumps(report,indent=2)+'\n')
print('Scoped150% and native reboot/count observations assessed; owned transports retired.')
