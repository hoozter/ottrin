"""Bind fresh mounted-AppImage actions and reviewed Services visuals to staged artifacts."""
from pathlib import Path
import hashlib,json,subprocess
r=Path.cwd();e=Path(__file__).resolve().parent;out=e/'ubuntu'
read=lambda p:json.loads(p.read_text())
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
source=read(e/'BUILD_SOURCE.json')['source_revision'];content=read(e/'PACKAGE_CONTENT_ACCEPTANCE.json');image=read(e/'QA_APPIMAGE_INPUT.json')
assert source==content['source_revision'] and image['sha256']==content['artifacts'][image['name']]
vm=read(out/'VM_RESULT.json');actor=read(out/'results/RESULT.json');owner=read(out/'VM_OWNER.json');cleanup=read(out/'VM_CLEANUP.json')
assert vm['status']=='pass within actor scope' and actor['status']=='pass' and actor['source']==vm['source']==source
assert actor['launch_kind']=='AppImage/FUSE' and actor['worker_exit']==0 and actor['private_password_removed']
worker=actor['worker'];assert worker['status']=='pass' and worker['source_preserved'] and len(worker['cases'])==3
assert ' - fuse.' in worker['fuse_mount']['mountinfo']
assert owner['actor_sha256']==sha(e/'ACTOR_SOURCE.py') and owner['controller_sha256']==sha(e/'CONTROLLER_SOURCE.py')
states=worker['service_states'];assert len(states)==9
assert states[0]['state']['pid']==states[1]['state']['pid']==states[2]['state']['pid']>0
assert all(s['state']['pid']==0 for s in states[3:7])
assert states[6]['state']['enabled']=='disabled' and states[7]['state']['pid']>0
assert states[8]['state']['pid']==0 and not states[8]['state']['installed']
close=worker['gui_close_and_reopen'];assert close['normal_exit']==0 and close['after_close']==close['after_reopen']
assert close['after_close']['pid']==states[7]['state']['pid']>0 and close['after_close']['enabled']=='disabled'
assert cleanup['domain_removed'] and cleanup['volumes_removed']==owner['volumes'] and vm['temporary_seed_and_ssh_keys_removed']
assert vm['uuid'] not in subprocess.check_output(['virsh','-c','qemu:///system','list','--all','--uuid'],text=True).split()
assert not list(out.glob('.native-vm-*')) and not (out/'VM_RESULTS.tar.gz').exists()
review={
 'setup-needed':'Direct lock route reaches Services; setup needed/action visible.',
 'cancel-prompt':'Real graphical polkit prompt for staged helper.',
 'setup-cancelled':'Cancellation visible; helper remains uninstalled.',
 'setup-prompt':'Retry opens a new graphical administrator prompt.',
 'setup-ready':'Root-owned helper is reflected by green ready status.',
 'reinstall-prompt':'Reinstall prompts after temporary authorization is revoked.',
 'reinstall-ready':'Reinstall completes and ready status remains.',
 'index-running-startup-on':'Green running state, checked startup choice, update and uninstall controls.',
 'index-stopped-startup-off':'ACCEPTED TARGET: gray Stopped with disabled startup, no red connection-refused text in Services.',
 'index-uninstalled':'ACCEPTED TARGET: gray Stopped and Enable automatic startup after uninstall; no false Services error.',
 'index-reopened':'DIAGNOSTIC ONLY: running/disabled state visible but immediate post-resize image includes an incomplete white region; not accepted as a settled full-window/theme capture.'}
for name in review:assert (out/'results'/(name+'.png')).is_file()
report={'status':'pass scoped fresh AppImage Services stopped-state, helper and native control acceptance','source_revision':source,'appimage':image,'service_states':9,'startup_checkbox_checks':5,'gui_close_and_reopen':close,'helper_cases':worker['cases'],'source_preserved':True,'reviewed_original_captures':{name:{'sha256':sha(out/'results'/(name+'.png')),'review':note} for name,note in review.items()},'limits':['Private Ubuntu24 software-X11/PAM/GNOME-polkit fixture only; no physical, browser or whole-interface acceptance','No protected-file elevation or real login/reboot acceptance','Fresh normal GUI close/reopen and PID retention accepted; post-resize screenshot is diagnostic, not settled-window acceptance'],'remaining_ui_audit_findings':['Main-window search indicator remains red when indexing is intentionally stopped; Services presentation is corrected, whole-interface status coherence is unfinished','Immediate post-resize capture is incomplete; verify settled repaint and fix producer observation before treating resize captures as acceptance'],'cleanup':cleanup}
(e/'LINUX_NATIVE_ACCEPTANCE.json').write_text(json.dumps(report,indent=2)+'\n');print(report['status'])
