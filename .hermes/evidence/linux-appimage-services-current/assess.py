"""Separate accepted AppImage actions from the directly observed stopped-state UI defect."""
from pathlib import Path
import ast,hashlib,json,subprocess
r=Path.cwd();root=Path(__file__).resolve().parent;out=root/'ubuntu'
read=lambda p:json.loads(p.read_text())
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
manifest=read(root/'PACKAGE_INPUT.json');image=read(root/'QA_APPIMAGE_INPUT.json')
assert manifest['artifacts'][image['name']]==image['sha256']==sha(r/'dist/packages'/image['name'])
owner=read(out/'VM_OWNER.json');vm=read(out/'VM_RESULT.json');actor=read(out/'results/RESULT.json');cleanup=read(out/'VM_CLEANUP.json')
assert vm['status']=='pass within actor scope' and actor['status']=='pass' and actor['worker_exit']==0
assert actor['launch_kind']=='AppImage/FUSE' and actor['source']==vm['source']==manifest['source_revision']
worker=actor['worker'];assert worker['status']=='pass' and len(worker['cases'])==3 and worker['source_preserved']
assert ' - fuse.' in worker['fuse_mount']['mountinfo'] and '/tmp/.mount_' in worker['fuse_mount']['executable']
assert owner['actor_sha256']==sha(root/'ACTOR_SOURCE.py') and owner['controller_sha256']==sha(root/'CONTROLLER_SOURCE.py')
assert read(out/'GUEST_INPUT_HASHES.json')=={'actor.py':sha(root/'ACTOR_SOURCE.py'),'prompt-worker.py':sha(root/'WORKER_SOURCE.py'),'linux-desktop-qa.py':sha(root/'DESKTOP_QA_SOURCE.py'),'QA_APPIMAGE_INPUT.json':sha(root/'QA_APPIMAGE_INPUT.json'),image['name']:image['sha256']}
states=worker['service_states'];assert len(states)==9
assert states[0]['state']['pid']==states[1]['state']['pid']==states[2]['state']['pid']>0
assert all(s['state']['pid']==0 for s in states[3:7])
assert states[6]['state']['enabled']=='disabled' and states[7]['state']['pid']>0
assert not states[8]['state']['installed'] and states[8]['state']['pid']==0
review={
 'setup-needed':'Setup-needed state and direct setup action visible.',
 'cancel-prompt':'Real graphical administrator prompt for staged helper from the mounted image.',
 'setup-cancelled':'Cancellation is visible; setup-needed state remains.',
 'setup-prompt':'Retry opens a new real administrator prompt.',
 'setup-ready':'Green helper-ready state after authenticated setup.',
 'reinstall-prompt':'Reinstall prompts again after revoking temporary authorization.',
 'reinstall-ready':'Reinstall completes with helper-ready status.',
 'index-running-startup-on':'Managed service running; startup checked; update and uninstall actions replace install.',
 'index-stopped-startup-off':'VISUAL FAILURE: service is disabled/stopped but red Needs attention and connection-refused text are displayed.',
 'index-uninstalled':'VISUAL FAILURE: uninstall succeeds but stale socket makes the stopped index appear broken.'}
assert cleanup['domain_removed'] and cleanup['volumes_removed']==owner['volumes']
assert actor['private_password_removed'] and vm['temporary_seed_and_ssh_keys_removed']
uuids=subprocess.check_output(['virsh','-c','qemu:///system','list','--all','--uuid'],text=True).split();assert vm['uuid'] not in uuids
assert not list(root.rglob('.native-vm-*')) and not list(root.rglob('VM_RESULTS.tar.gz'))
for p in [root/'ACTOR_SOURCE.py',root/'WORKER_SOURCE.py',root/'CONTROLLER_SOURCE.py',Path(__file__)]:ast.parse(p.read_text())
report={'status':'AppImage FUSE helper setup and service actions pass; stopped-state visual acceptance fails','runtime_source':manifest['source_revision'],'appimage':image,'helper_cases':worker['cases'],'fuse_mount':worker['fuse_mount'],'native_service_states':states,'original_captures_directly_reviewed':{name:{'sha256':sha(out/'results'/(name+'.png')),'review':note} for name,note in review.items()},'source_fix':{'scope':'Nonblocking Linux endpoint-presence probe distinguishes live/busy listener, refused stale socket and invalid files; Services uses it for stopped-state presentation','delivery':'Source correction requires new Linux packages and native stopped/uninstalled screenshots before acceptance'},'limits':['Ubuntu24 private software X11 with stock GNOME polkit agent only','No protected-file elevation, normal GUI-close retention, real login/reboot or binary-version upgrade acceptance','No browser installed; HTML rendering acceptance is separate'],'cleanup':{'owned_vm_and_three_volumes_removed':True,'private_password_keys_seeds_and_transport_archive_removed':True,'retained_path':str(root),'purpose':'Original captures, frozen producers/configuration, exact package/input hashes, source-check logs and explicit visual failure; no packages or VM disks'}}
(root/'ACCEPTANCE.json').write_text(json.dumps(report,indent=2)+'\n')
print(report['status'])
