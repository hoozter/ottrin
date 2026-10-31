"""Promote only after exact package/UI acceptance and complete guest teardown."""
from pathlib import Path
import json,hashlib,subprocess,os,shutil,zipfile
r=Path.cwd();p=Path(__file__).resolve().parent;g=p.parent/'windows-copy-undo-gui-current'
stage=r/'.hermes/packaging-tmp/.windows-ci-stage';target=r/'dist/windows-candidate'
def read(path):return json.loads(path.read_text(encoding='utf-8-sig'))
def sha(path):
 with path.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
prior=read(r/'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json');old=Path(prior['candidate']);backup=old.with_name('.windows-activity-superseded')
assert old==r/'.hermes/packaging-tmp/windows-activity-candidate'
assert not target.exists() and not backup.exists()
assert subprocess.check_output(['virsh','-c','qemu:///system','domstate','ottrin-copy-cleanup-qa'],text=True).strip()=='shut off'
# Other projects may compile independently; only Ottrin consumers can block
# this cohort transition. All artifact and expanded-payload handles are checked below.
for pid in subprocess.run(['pgrep','-x','cargo|rustc'],capture_output=True,text=True).stdout.split():
 try:
  cwd=Path(os.readlink('/proc/'+pid+'/cwd')).resolve()
  arguments=Path('/proc/'+pid+'/cmdline').read_bytes()
 except FileNotFoundError:continue
 assert not cwd.is_relative_to(r) and str(r).encode() not in arguments,('Ottrin compiler remains active',pid)
cohort=read(p/'HOST_PAYLOAD.json');gui=read(g/'GUI_SCOPES.json');hosted=read(p/'HOSTED_CURRENT.json')
assert read(g/'SESSION.json')['status']=='pass' and read(p/'NATIVE_OWNER.json')['status']=='pass'
assert gui['source']==cohort['source_revision']==hosted['headSha'] and hosted['conclusion']=='success'
assert gui['status']=='pass' and gui['normal_close']['exit']==0
assert gui['old_path_occupants_preserved'] and gui['single_click_undo_verified'] and gui['batch_click_undo_verified']
for directory,hashes in [(old,prior['artifacts']),(stage,cohort['artifacts'])]:
 assert {f.name for f in directory.iterdir()}==set(hashes)|{n+'.sha256' for n in hashes}|{'windows-payload'}
 for name,digest in hashes.items():
  assert sha(directory/name)==digest and (directory/(name+'.sha256')).read_text().split()==[digest,name]
 with zipfile.ZipFile(directory/'Ottrin-0.1.0-windows-x64-portable.zip') as z:
  expected={f.filename:hashlib.sha256(z.read(f)).hexdigest() for f in z.infolist() if not f.is_dir()}
 actual={str(f.relative_to(directory/'windows-payload')):sha(f) for f in (directory/'windows-payload').rglob('*') if f.is_file()}
 assert expected==actual
 files=[str(f) for f in directory.rglob('*') if f.is_file()]
 assert not any(f.is_symlink() for f in directory.rglob('*'))
 assert subprocess.run(['fuser',*files],capture_output=True).returncode==1
receipt={'status':'pass within exact payload,native loader/CLI,scoped ordinary portable GUI and complete hosted source verification',
 'source_commit':cohort['source_revision'],'working_tree_at_build':'clean','candidate':str(target),'artifacts':cohort['artifacts'],'payload_files':58,
 'payload_identity':str((p/'PAYLOAD_GUI_MANIFEST.json').relative_to(r)),
 'evidence_directory':str(p),'native_msi_administrative_extraction':read(p/'MSI_PAYLOAD.json'),'native_loader_cli':read(p/'CLI_SMOKE.json'),
 'portable_gui_acceptance':gui,'source_checks':'.hermes/evidence/windows-copy-undo-admission-current/ACCEPTANCE.json',
 'hosted_source':{'run':40295923460,'revision':cohort['source_revision'],'conclusion':'success','evidence':str((p/'HOSTED_CURRENT.json').relative_to(r))},
 'cleanup':{'native_package':'windows-copy-undo-package-current/NATIVE_OWNER.json','portable_gui':'windows-copy-undo-gui-current/SESSION.json','guest_off':True,'original_guest_files_and_profiles_preserved':True,'account_profile_credential_task_and_extractions_removed':True},
 'public_windows_files_unchanged':True,'linux_packages_unchanged':True,
 'limits':['No install/upgrade/repair/rollback/uninstall acceptance for these hashes; MSI rollback remains required and unresolved','GUI scope ordinary Dark/List100/default renderer1024x680 on1280x800 NTFS VM','Earlier cross-volume,Office,appearance and physical evidence retain original hashes','No production release,Store submission or final owner acceptance']}
(p/'PREVIOUS_ACCEPTANCE.json').write_text(json.dumps(prior,indent=2)+'\n')
os.replace(old,backup)
try:
 os.replace(stage,target)
 for name,digest in cohort['artifacts'].items():assert sha(target/name)==digest
 temporary=r/'dist/.WINDOWS_CANDIDATE_ACCEPTANCE.next';temporary.write_text(json.dumps(receipt,indent=2)+'\n');os.replace(temporary,r/'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json')
except BaseException:
 if target.exists():os.replace(target,stage)
 os.replace(backup,old);raise
size=sum(f.stat().st_size for f in backup.rglob('*') if f.is_file());shutil.rmtree(backup)
(p/'PROMOTION.json').write_text(json.dumps({'status':'pass','stable_candidate':str(target),'previous_stable_candidate':str(old),'source':cohort['source_revision'],'superseded_bytes_removed':size,'old_directory_removed':not old.exists(),'stage_removed':not stage.exists(),'reason_for_stable_path':'Updated project instruction requires current deliverables under dist; this is the single internal Windows cohort,not a public release'},indent=2)+'\n')
source=read(p/'BUILD_SOURCE.json');source.update(pending_consumers=[],final_candidate=str(target),cleanup_complete=True);(p/'BUILD_SOURCE.json').write_text(json.dumps(source,indent=2)+'\n')
print('Promoted exact verified cohort to',target,'; retired',size,'superseded bytes')
