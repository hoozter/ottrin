"""Bind exact packaged Copy/Undo/cleanup acceptance to native/root evidence."""
from pathlib import Path
import hashlib,json,subprocess
r=Path.cwd();e=Path(__file__).resolve().parent;d=e/'native-upgrade';out=d/'ubuntu';read=lambda p:json.loads(p.read_text());sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
build=read(e/'BUILD_RESULT.json');image=read(d/'QA_APPIMAGE_INPUT.json');v=read(out/'VM_RESULT.json');a=read(out/'results/RESULT.json');w=read(out/'results/WORKER_RESULT.json');cleanup=read(out/'VM_CLEANUP.json');owner=read(out/'VM_OWNER.json')
assert v['status']=='pass within actor scope' and a['status']==w['status']=='pass'
assert v['source']==a['source']==build['source_revision']
assert image['sha256']==build['artifacts'][image['name']] and v['package_sha256']==build['artifacts']['ottrin_0.1.0_amd64.deb']
assert sha(d/'actor.py')==owner['actor_sha256'] and sha(d/'controller.py')==owner['controller_sha256']
assert 'Seat=seat0\n' in w['session_properties'] and 'Active=yes\n' in w['session_properties'] and 'Class=user\n' in w['session_properties']
assert w['protected_helper']['ordinary_write_denied'] and w['protected_helper']['cancel_exit']==126 and w['protected_helper']['authenticated_response']['status']=='Success'
assert len(w['protected_helper']['authenticated_response']['payload']['Copy']['transactions'])==1
assert w['protected_helper']['undo_and_cleanup']=='Success through fresh authenticated helper processes'
for name in ['helper_copy_root_verification','gui_copy_root_verification']:
 assert w[name]['root_owned'] and w[name]['exact_source_bytes']
assert w['gui_privileged_copy'].startswith('Actual cp command denied, authenticated app retry') and w['source_preserved']
assert a['protected_copy']=={'root_owned_copies_match_source':['GUI_COPY','HELPER_COPY'],'undo_removed_publications':True,'explicit_cleanup_removed_stages':True,'cancelled_destination_absent':True}
assert a['owned_protected_directory_removed'] and a['private_password_removed'] and v['temporary_seed_and_ssh_keys_removed']
assert cleanup['domain_removed'] and len(cleanup['volumes_removed'])==3
assert subprocess.run(['virsh','-c','qemu:///system','domuuid',owner['domain']],capture_output=True).returncode!=0
for volume in cleanup['volumes_removed']:assert subprocess.run(['virsh','-c','qemu:///system','vol-path',volume,'--pool','default'],capture_output=True).returncode!=0
captures=['old-helper-update-needed.png','update-cancel-prompt.png','update-cancelled.png','update-prompt.png','updated-helper-ready.png','gui-copy-denied.png','gui-copy-prompt.png','gui-copy-activity.png','gui-undo-prompt.png','gui-undo-completed.png','gui-cleanup-prompt.png','gui-copy-cleanup-completed.png']
review=read(e/'CAPTURE_REVIEW.json')
assert review['scope']=='Original complete-window captures directly reviewed'
for name in captures:assert review['captures'][name]==sha(out/'results'/name)
assert w['update_guidance']=='Administrator helper needs an update. Open Services and choose Update / reinstall helper.'
old=read(d/'QA_OLD_HELPER_INPUT.json')
assert old['sha256']==a['old_helper_sha256']
assert w['updated_helper_sha256']==sha(r/'.hermes/packaging-tmp/.linux-desktop-extract/squashfs-root/usr/libexec/ottrin/ottrin-priv-helper')
assert w['updated_helper_sha256']!=old['sha256']
assert any('cancel preserves exact old helper' in case for case in w['cases'])
assert any('actual Ready status' in case for case in w['cases'])
report={'status':'pass exact AppImage/FUSE old-helper upgrade/cancel/readiness and application Copy recovery scope','source_revision':build['source_revision'],'artifact':image['name'],'artifact_sha256':image['sha256'],'session':w['session_properties'],'old_helper_sha256':old['sha256'],'updated_helper_sha256':w['updated_helper_sha256'],'helper_cases':w['cases'],'protected_helper':w['protected_helper'],'actual_application':w['gui_privileged_copy'],'independent_root_proofs':{key:w[key] for key in ['helper_copy_root_verification','gui_copy_root_verification']},'root_final_state':a['protected_copy'],'source_preserved':True,'original_captures_directly_reviewed':{str((out/'results'/name).relative_to(r)):sha(out/'results'/name) for name in captures},'cleanup':cleanup,'fixture_environment':'Ubuntu24 actual LightDM seat0, Openbox, stock GNOME polkit agent, software virtual graphics; explicit desktop dependencies without unrelated desktop recommendations','limits':['Same-session Linux Copy and root-owned disposable destination only; source-level changed-object/refusal/partial/interrupted-cleanup tests have separate scopes','Windows privileged Copy and ordinary-user elevated-account Copy discovery after application restart remain open','No real Linux reboot/login, full upgrade matrix, physical graphics, complete theme audit or daily-driver acceptance']}
archive=out/'VM_RESULTS.tar.gz';report['consumed_transport_retired']={'sha256':sha(archive),'bytes':archive.stat().st_size};archive.unlink()
(e/'LINUX_PRIVILEGED_COPY_ACCEPTANCE.json').write_text(json.dumps(report,indent=2)+'\n');print(report['status'])
