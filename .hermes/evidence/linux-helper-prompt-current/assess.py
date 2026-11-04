"""Verify exact published portable prompt evidence and completed resource cleanup."""
from pathlib import Path
import ast,hashlib,json,subprocess
r=Path.cwd();root=Path(__file__).resolve().parent;out=root/'ubuntu'
read=lambda p:json.loads(p.read_text())
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
manifest=read(root/'PACKAGE_INPUT.json');current=read(r/'dist/packages/LINUX_PACKAGES_ACCEPTANCE.json')
assert manifest['artifacts']==current['artifacts'] and manifest['source_revision']==current['source_revision']
portable=next(name for name in manifest['artifacts'] if name.endswith('portable.tar.gz'))
assert sha(r/'dist/packages'/portable)==manifest['artifacts'][portable]
owner=read(out/'VM_OWNER.json');vm=read(out/'VM_RESULT.json');actor=read(out/'results/RESULT.json');cleanup=read(out/'VM_CLEANUP.json')
assert vm['status']=='pass within actor scope' and actor['status']=='pass' and actor['worker_exit']==0
assert vm['source']==actor['source']==manifest['source_revision']
assert actor['worker']['status']=='pass' and len(actor['worker']['cases'])==3
assert actor['worker']['source_preserved'] and actor['worker']['login_session_id']
assert actor['private_password_removed'] and vm['temporary_seed_and_ssh_keys_removed']
assert owner['controller_sha256']==sha(root/'CONTROLLER_SOURCE.py')
assert owner['actor_sha256']==sha(root/'desktop-actor.py')==sha(out/'ACTOR_SOURCE.py')
inputs=read(out/'GUEST_INPUT_HASHES.json')['verified_guest_input_hashes']
assert inputs=={'actor.py':sha(root/'desktop-actor.py'),'prompt-worker.py':sha(root/'prompt-worker.py'),'linux-desktop-qa.py':sha(root/'DESKTOP_QA_SOURCE.py')}
assert sha(root/'prompt-worker.py')==sha(out/'WORKER_SOURCE.py')
assert cleanup['domain_removed'] and cleanup['volumes_removed']==owner['volumes']
failed=root/'stale-lease-rejected';failed_cleanup=read(failed/'VM_CLEANUP.json')
assert read(failed/'VM_RESULT.json')['status']=='fail' and failed_cleanup['domain_removed']
assert failed_cleanup['volumes_removed']==read(failed/'VM_OWNER.json')['volumes']
assert read(failed/'VM_OWNER.json')['controller_sha256']==sha(failed/'CONTROLLER_SOURCE.py')
assert not list(root.rglob('.native-vm-*')) and not list(root.rglob('VM_RESULTS.tar.gz'))
names=subprocess.check_output(['virsh','-c','qemu:///system','list','--all','--name'],text=True).split()
assert not any(n.startswith('ottrin-native-') for n in names)
review={
 'setup-needed':'Services is reached from the lock; setup needed and setup action visible.',
 'cancel-prompt':'Actual stock polkit agent presents Cancel and Authenticate for the staged helper.',
 'setup-cancelled':'Cancellation message and setup-needed state remain; helper is not claimed ready.',
 'setup-prompt':'Retry presents a new real graphical administrator-password prompt.',
 'setup-ready':'Green installed-and-ready helper status and update/reinstall action; absent browser remains correctly reported.',
 'reinstall-prompt':'Reinstall presents another real authentication prompt after revoking temporary authorization.',
 'reinstall-ready':'Reinstall completed; installed-and-ready helper status and usable controls visible.'}
for name in review:assert (out/'results'/(name+'.png')).is_file()
for p in [root/'CONTROLLER_SOURCE.py',root/'desktop-actor.py',root/'prompt-worker.py',Path(__file__)]:ast.parse(p.read_text())
report={'status':'pass scoped real graphical helper setup/cancel/reinstall','runtime_source':manifest['source_revision'],'portable':{'name':portable,'sha256':manifest['artifacts'][portable]},'native_worker':actor['worker'],'original_captures_directly_reviewed':{name:{'sha256':sha(out/'results'/(name+'.png')),'review':note} for name,note in review.items()},'limits':['Ubuntu24 stock GNOME polkit agent in private software X11/PAM login session only','Portable launcher tested; AppImage FUSE-specific setup remains separate','No native startup-checkbox, real login/reboot, binary-version upgrade or protected-file-operation acceptance','No supported browser was installed; HTML browser/rendering acceptance is not claimed'],'cleanup':{'two_owned_vms_and_six_volumes_removed':True,'temporary_qa_password_ssh_keys_and_seeds_removed':True,'transport_archives_removed':True,'retained_path':str(root),'purpose':'Compact original screenshots, frozen producers, exact package/input hashes, logs and stale-lease diagnosis; no package copies or VM disks'}}
(root/'ACCEPTANCE.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({'status':report['status'],'reviewed_captures':len(review),'cleanup':report['cleanup']},indent=2))
