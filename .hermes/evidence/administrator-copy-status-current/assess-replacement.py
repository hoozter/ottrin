"""Bind four real stable-path update cases to exact packages and owned teardown."""
from pathlib import Path
import hashlib,json,subprocess
R=Path.cwd();E=Path(__file__).resolve().parent;N=E/'replacement';out=N/'ubuntu';read=lambda p:json.loads(p.read_text());sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
build=read(E/'BUILD_RESULT.json');inputs=read(N/'REPLACEMENT_INPUT.json');vm=read(out/'VM_RESULT.json');owner=read(out/'VM_OWNER.json');cleanup=read(out/'VM_CLEANUP.json');actor=read(out/'results/RESULT.json');worker=read(out/'results/WORKER_RESULT.json')
assert vm['status']=='pass within actor scope' and actor['status']==worker['status']=='pass'
assert vm['source']==actor['source_revision']==build['source_revision']==inputs['new']['source_revision']
assert inputs['new']['portable_sha256']==build['artifacts']['Ottrin-0.1.0-linux-x86_64-portable.tar.gz']
assert inputs['new']['appimage_sha256']==build['artifacts']['Ottrin-0.1.0-x86_64.AppImage']
assert sha(N/'actor.py')==owner['actor_sha256'] and sha(N/'controller.py')==owner['controller_sha256']
assert len(worker['cases'])==4
for case in worker['cases']:
 enabled=case['case'].endswith('-enabled')
 assert case['before']['startup_enabled']==case['after']['startup_enabled']==enabled
 assert case['before']['daemon_sha256']==inputs['old']['daemon_sha256']
 assert case['after']['daemon_sha256']==inputs['new']['daemon_sha256']
 assert case['before']['daemon_sha256']!=case['after']['daemon_sha256']
 assert case['before']['pid']!=case['after']['pid']
 assert case['before']['gui_attempts']==case['after']['gui_attempts']==1
 assert case['before']['view_preference']==case['after']['view_preference']=='Grid'
 assert case['database_inode_preserved'] and case['sources_preserved'] and case['retired_payload_removed']
assert cleanup['domain_removed'] and len(cleanup['volumes_removed'])==3
assert subprocess.run(['virsh','-c','qemu:///system','domuuid',owner['domain']],capture_output=True).returncode!=0
for volume in cleanup['volumes_removed']:assert subprocess.run(['virsh','-c','qemu:///system','vol-path',volume,'--pool','default'],capture_output=True).returncode!=0
assert read(E/'REPLACEMENT_CONTROLLER.json')['owned_host_input_removed']
assert not any(out.glob('.native-vm-*'))
captures=[case['case']+'-'+version+'.png' for case in worker['cases'] for version in ['old','new']]
review=read(E/'REPLACEMENT_CAPTURE_REVIEW.json')
assert review['scope']=='Original complete-window captures directly reviewed'
for name in captures:assert review['captures'][name]==sha(out/'results'/name)
report={'status':'pass AppImage/portable stable-path binary replacement with startup enabled/disabled','source_revision':build['source_revision'],'inputs':inputs,'cases':worker['cases'],'scope':'Real Ubuntu24 LightDM/Openbox software-X11 windows: old f8 service and GUI, stable-path atomic payload replacement, first new GUI launch replaces daemon, same startup choice/database inode and Grid preference, normal close and supported service removal. No source files or personal profile used.','original_captures_directly_reviewed':{str((out/'results'/name).relative_to(R)):sha(out/'results'/name) for name in captures},'capture_notes':review.get('notes',[]),'cleanup':cleanup,'limits':['One Linux software graphics fixture and private100-file corpus; no Windows, physical desktop, full theme audit or large-corpus performance claim','Literal package version remains0.1.0; actual old/new daemon and GUI byte identities differ and are checked from mapped processes']}
archive=out/'VM_RESULTS.tar.gz';report['consumed_transport']={'bytes':archive.stat().st_size,'sha256':sha(archive)};archive.unlink()
(E/'REPLACEMENT_ACCEPTANCE.json').write_text(json.dumps(report,indent=2)+'\n');print(report['status'])
