"""Promote only after exact package/UI acceptance and complete guest teardown."""
from pathlib import Path
import json,hashlib,subprocess,os,shutil,zipfile
r=Path.cwd();p=Path(__file__).resolve().parent;g=p.parent/'windows-navigation-gui-current'
stage=r/'.hermes/packaging-tmp/.windows-ci-stage';target=r/'dist/windows-candidate'
def read(path):return json.loads(path.read_text(encoding='utf-8-sig'))
def sha(path):
 with path.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
prior=read(r/'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json');old=Path(prior['candidate']);backup=old.with_name('.windows-navigation-superseded')
assert old==target
assert target.is_dir() and not backup.exists()
assert subprocess.check_output(['virsh','-c','qemu:///system','domstate','ottrin-copy-cleanup-qa'],text=True).strip()=='shut off'
# Other projects may compile independently; only Ottrin consumers can block
# this cohort transition. All artifact and expanded-payload handles are checked below.
for pid in subprocess.run(['pgrep','-x','cargo|rustc'],capture_output=True,text=True).stdout.split():
 try:
  cwd=Path(os.readlink('/proc/'+pid+'/cwd')).resolve()
  arguments=Path('/proc/'+pid+'/cmdline').read_bytes()
 except FileNotFoundError:continue
 assert not cwd.is_relative_to(r) and str(r).encode() not in arguments,('Ottrin compiler remains active',pid)
cohort=read(p/'HOST_PAYLOAD.json');gui=read(g/'GUI_SCOPES.json');hosted=read(p/'HOSTED_SOURCE.json')
assert read(g/'SESSION.json')['status']=='pass' and read(p/'NATIVE_OWNER.json')['status']=='pass'
assert gui['source']==cohort['source_revision']==hosted['headSha'] and hosted['conclusion']=='success'
assert gui['status']=='pass' and gui['normal_close']['exit']==0
assert gui['view_switch_reveal_verified'] and gui['metadata_hover_verified'] and gui['readonly_navigation_verified']
assert read(g/'FINAL_RAW_BYTES.json')['status']=='pass'
assert len(read(g/'FINAL_RAW_BYTES.json')['files'])==5
assert gui['preferences_preserved']
assert subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()==cohort['source_revision']
changed=set(subprocess.check_output(['git','diff','--name-only','HEAD'],text=True).splitlines())
assert changed <= {'dist/packages/LINUX_PACKAGES_ACCEPTANCE.json'}, changed
# The preceding Linux promotion updates this tracked receipt, not build inputs.
linux=read(r/'dist/packages/LINUX_PACKAGES_ACCEPTANCE.json')
assert linux['source_revision']==cohort['source_revision']
assert linux==read(p.parent/'linux-navigation-package-current/ACCEPTANCE.json')
for name,digest in linux['artifacts'].items():assert sha(r/'dist/packages'/name)==digest
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
 assert all(Path(f).stat().st_nlink==1 for f in files)
 assert subprocess.run(['fuser',*files],capture_output=True).returncode==1
receipt={'status':'pass within exact payload,native loader/CLI,scoped ordinary portable GUI and complete hosted source verification',
 'source_commit':cohort['source_revision'],'working_tree_at_build':'clean','candidate':str(target),'artifacts':cohort['artifacts'],'payload_files':58,
 'payload_identity':str((p/'PAYLOAD_GUI_MANIFEST.json').relative_to(r)),
 'evidence_directory':str(p),'native_msi_administrative_extraction':read(p/'MSI_PAYLOAD.json'),'native_loader_cli':read(p/'CLI_SMOKE.json'),
 'portable_gui_acceptance':gui,'source_checks':'.hermes/evidence/navigation-toolbar-current/SOURCE_CHECKS.json',
 'hosted_source':{'run':41251463739,'revision':cohort['source_revision'],'conclusion':'success','evidence':str((p/'HOSTED_SOURCE.json').relative_to(r))},
 'cleanup':{'native_package':'windows-navigation-package-current/NATIVE_OWNER.json','portable_gui':'windows-navigation-gui-current/SESSION.json','guest_off':True,'original_guest_files_and_profiles_preserved':True,'account_profile_credential_task_and_extractions_removed':True},
 'public_windows_files_unchanged':True,'linux_source_cohort':'e79b915e;accepted separately at dist/packages',
 'limits':['No install/upgrade/repair/rollback/uninstall acceptance for these hashes; MSI rollback remains required and unresolved','GUI scope ordinary Dark100/default launch1024x680 on1280x800 NTFS VM;List/Grid to Column,metadata,read-only navigation/Space,normal Close and five final raw files','Earlier cross-volume,Office,appearance and physical evidence retain original hashes','No production release,Store submission or final owner acceptance']}
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
(p/'PROMOTION.json').write_text(json.dumps({'status':'pass','stable_candidate':str(target),'previous_stable_candidate':str(old),'source':cohort['source_revision'],'superseded_bytes_removed':size,'superseded_directory_removed':not backup.exists(),'stage_removed':not stage.exists(),'reason_for_stable_path':'Replace the one internal Windows cohort under dist;preserve public release files and existing user data'},indent=2)+'\n')
source=read(p/'BUILD_SOURCE.json');source.update(pending_consumers=[],final_candidate=str(target),cleanup_complete=True);(p/'BUILD_SOURCE.json').write_text(json.dumps(source,indent=2)+'\n')
print('Promoted exact verified cohort to',target,'; retired',size,'superseded bytes')
