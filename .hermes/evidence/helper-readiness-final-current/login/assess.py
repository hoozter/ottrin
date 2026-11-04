"""Bind actual desktop-login and reboot results to native package and teardown."""
from pathlib import Path
import hashlib,json,subprocess,uuid
R=Path.cwd();E=Path(__file__).resolve().parent;out=E/'ubuntu';read=lambda p:json.loads(p.read_text());sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
build=read(E/'BUILD_RESULT.json');vm=read(out/'VM_RESULT.json');owner=read(out/'VM_OWNER.json');cleanup=read(out/'VM_CLEANUP.json')
assert vm['status']=='pass within actor scope' and vm['source']==build['source_revision']
assert vm['package_sha256']==build['artifacts']['ottrin_0.1.0_amd64.deb']
assert len(vm['boot_ids'])==len(set(vm['boot_ids']))==3
for boot in vm['boot_ids']:uuid.UUID(boot)
assert sha(E/'actor.py')==owner['actor_sha256'] and sha(E/'controller.py')==owner['controller_sha256']
phases=[read(out/'results'/f'PHASE{i}.json') for i in range(3)]
roots=[read(out/'results'/f'ROOT_PHASE{i}.json') for i in range(3)]
for i,(phase,root) in enumerate(zip(phases,roots)):
 assert phase['status']==root['status']=='pass' and root['source_revision']==build['source_revision']
 assert root['worker']==phase and phase['boot_id']==vm['boot_ids'][i]
 assert phase['gui_attempts']==1 and phase['source_and_preferences_preserved']
 assert 'Seat=seat0\n' in phase['session'] and 'Service=lightdm-autologin\n' in phase['session']
 assert phase['after_normal_gui_close_unit']['MainPID']==phase['after_gui_unit']['MainPID']
 assert phase['after_normal_gui_close_unit']['ActiveState']=='active'
 assert roots[i]['control_account']!='ottrinqa' and roots[i]['desktop_account']=='ottrinqa'
assert phases[0]['startup_enabled'] and not phases[1]['startup_enabled'] and not phases[2]['startup_enabled']
assert int(phases[1]['before_gui_auto_started_unit']['MainPID'])>0
assert phases[1]['before_gui_unit']['MainPID']==phases[1]['after_gui_unit']['MainPID']
assert phases[2]['disabled_before_gui_no_daemon']
assert len({p['database_inode'] for p in phases})==1
comparison=read(E.parent/'PAYLOAD_COMPARISON.json')
assert all(p['daemon_sha256']==comparison['daemon_sha256'] for p in roots)
assert cleanup['domain_removed'] and len(cleanup['volumes_removed'])==3
assert subprocess.run(['virsh','-c','qemu:///system','domuuid',owner['domain']],capture_output=True).returncode!=0
for p in E.rglob('VM_CLEANUP.json'):
 c=read(p);assert c['domain_removed'] and len(c['volumes_removed'])==3
 for volume in c['volumes_removed']:assert subprocess.run(['virsh','-c','qemu:///system','vol-path',volume,'--pool','default'],capture_output=True).returncode!=0
assert not list(E.rglob('.native-vm-*')) and not list(E.rglob('id_ed25519')) and not list(E.rglob('VM_RESULTS.tar.gz'))
captures=['main-phase0.png','disabled-login-running-index.png','main-phase1.png','main-phase2.png']
report={'status':'pass scoped native DEB actual desktop login, two real reboots and GUI-close index lifetime','source_revision':build['source_revision'],'package_sha256':vm['package_sha256'],'boot_ids':vm['boot_ids'],'phases':phases,'daemon_sha256':roots[0]['daemon_sha256'],'control_account_separate_from_desktop_user':True,'database_inode_preserved':True,'selected_preferences_and1000_source_files_preserved':True,'original_captures_directly_reviewed':{str((out/'results'/name).relative_to(R)):sha(out/'results'/name) for name in captures},'capture_notes':['Phase2 initial capture still shows0 items/red search while its independent backend is Ready and file rows are visible. Settled startup status remains a U08 observation; this gate covers lifetime/login, not complete interface consistency.','Disabled checkbox and running index match actual unit state; wording Running but disabled in the automatic-startup label remains a U08 clarity improvement.'],'cleanup':cleanup,'failed_trial_cleanup_verified':0,'worker_host_source_sha256_at_assessment':sha(E/'worker.py'),'limits':['Ubuntu24 LightDM/Openbox software X11, native DEB and private1000-file fixture only','Does not cover AppImage/portable binary-version replacement, Wayland/physical graphics, Windows, large-corpus resource usage or full theme/UI audit','41c hosted test-scope failure is separately corrected; this native outcome is not a hosted release pass']}
(E/'ACCEPTANCE.json').write_text(json.dumps(report,indent=2)+'\n');print(report['status'])
