"""Assess exact AppImage browser/picker captures and registered guest teardown."""
from pathlib import Path
import hashlib,json,subprocess
R=Path.cwd();E=Path(__file__).resolve().parent;N=E/'native/ubuntu';out=N/'results'
read=lambda p:json.loads(p.read_text())
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
build=read(E/'BUILD_RESULT.json');content=read(E/'PACKAGE_CONTENT_ACCEPTANCE.json');actor=read(out/'RESULT.json');worker=read(out/'WORKER_RESULT.json');owner=read(N/'VM_OWNER.json');vm=read(N/'VM_RESULT.json');cleanup=read(N/'VM_CLEANUP.json');controller=read(E/'NATIVE_CONTROLLER.json');review=read(E/'CAPTURE_REVIEW.json')
assert actor['status']==worker['status']=='pass' and vm['status']=='pass within actor scope'
assert actor['package_source']==build['source_revision']==content['source_revision']==vm['source']==controller['source_revision']
assert sha(E/'native/actor.py')==owner['actor_sha256']
assert sha(Path(owner['cleanup_owner']))==owner['controller_sha256']
assert actor['appimage_input']['sha256']==build['artifacts']['Ottrin-0.1.0-x86_64.AppImage']
assert actor['appimage_input']['gui_sha256']==content['runtime_sha256']['application']
assert [(x['theme'],x['scale']) for x in worker['grid_cases']]==[('Dark',1.0),('Light',1.0),('Dark',.75),('Dark',1.85)]
assert [x['theme'] for x in worker['picker_cases']]==['Dark','Light'] and all(x['cancelled'] for x in worker['picker_cases'])
assert all(x['gui_sha256']==content['runtime_sha256']['application'] for x in worker['grid_cases']+worker['picker_cases'])
assert len(worker['startup_samples'])==4 and all(x['item_labels']==['label | 1002 items'] for x in worker['startup_samples'])
assert worker['startup_samples'][-1]['seconds_after_window']>=30
assert review['scope']=='Original complete-window captures directly reviewed'
names=['cold-'+str(i)+'.png' for i in [0,2,10,30]]+[x['capture'] for x in worker['grid_cases']+worker['picker_cases']]
assert len(names)==10
for name in names:assert review['captures'][name]==sha(out/name)
assert controller['child_exit']==0 and cleanup['domain_removed'] and len(cleanup['volumes_removed'])==3
assert subprocess.run(['virsh','-c','qemu:///system','domuuid',owner['domain']],capture_output=True).returncode!=0
for name in cleanup['volumes_removed']:assert subprocess.run(['virsh','-c','qemu:///system','vol-path',name,'--pool','default'],capture_output=True).returncode!=0
assert not list(N.glob('.native-vm-*'))
archive=N/'VM_RESULTS.tar.gz';transport={'bytes':archive.stat().st_size,'sha256':sha(archive)};archive.unlink()
report={'status':'pass scoped actual AppImage/FUSE browser and cooperating-picker Grid caption acceptance','source_revision':build['source_revision'],'artifact':'Ottrin-0.1.0-x86_64.AppImage','artifact_sha256':build['artifacts']['Ottrin-0.1.0-x86_64.AppImage'],'application_sha256':content['runtime_sha256']['application'],'grid_cases':worker['grid_cases'],'picker_cases':worker['picker_cases'],'cold_startup_observation':worker['startup_samples'],'cold_scope':worker['cold_scope'],'original_captures_directly_reviewed':{str((out/name).relative_to(R)):sha(out/name) for name in names},'capture_notes':review['notes'],'cleanup':cleanup,'consumed_transport_removed':transport,'limits':['Software X11/Ubuntu only; no physical graphics, Wayland, Windows or complete theme-editor audit.','Local cooperating CLI picker, browsing and cancellation; no new browser/portal/folder-integration acceptance.','Fresh-account count remained correct through30s; earlier installed-but-disabled initial-frame finding is not closed.','Native Miller wide-word caption underuses its row width; a separate follow-up is required.','Original unchanged helper/service/native recovery scopes remain bound to DAC in SOURCE_SCOPE.json; not freshly rerun.']}
(E/'NATIVE_GRID_ACCEPTANCE.json').write_text(json.dumps(report,indent=2)+'\n');print(report['status'])
