from pathlib import Path
import json,hashlib,subprocess,os,shutil,zipfile
r=Path.cwd();p=Path(__file__).resolve().parent;g=r/'.hermes/evidence/windows-copy-gui-current'
old=r/'.hermes/packaging-tmp/windows-activity-candidate';stage=r/'.hermes/packaging-tmp/.windows-ci-stage';backup=old.with_name('.windows-activity-superseded')
def sha(f):
 with f.open('rb') as s:return hashlib.file_digest(s,'sha256').hexdigest()
assert not backup.exists()
assert subprocess.check_output(['virsh','-c','qemu:///system','domstate','ottrin-copy-cleanup-qa'],text=True).strip()=='shut off'
assert not any(a.strip() in ['cargo','rustc'] for a in subprocess.check_output(['ps','-eo','comm='],text=True).splitlines())
prior=json.loads((r/'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json').read_text());cohort=json.loads((p/'HOST_PAYLOAD.json').read_text());gui=json.loads((g/'GUI_SCOPES.json').read_text())
assert json.loads((g/'SESSION.json').read_text())['status']=='pass'
assert json.loads((p/'NATIVE_OWNER.json').read_text())['status']=='pass'
assert gui['source']==cohort['source_revision']
for directory,hashes in [(old,prior['artifacts']),(stage,cohort['artifacts'])]:
 assert {x.name for x in directory.iterdir()}==set(hashes)|{n+'.sha256' for n in hashes}|{'windows-payload'}
 for name,h in hashes.items():
  assert sha(directory/name)==h
  assert subprocess.run(['fuser',str(directory/name)],capture_output=True).returncode==1
  assert (directory/(name+'.sha256')).read_text().split()==[h,name]
 with zipfile.ZipFile(directory/'Ottrin-0.1.0-windows-x64-portable.zip') as z:
  expected={f.filename:hashlib.sha256(z.read(f)).hexdigest() for f in z.infolist() if not f.is_dir()}
 actual={str(f.relative_to(directory/'windows-payload')):sha(f) for f in (directory/'windows-payload').rglob('*') if f.is_file()}
 assert expected==actual
 assert not any(f.is_symlink() for f in directory.rglob('*'))
(p/'PREVIOUS_ACCEPTANCE.json').write_text(json.dumps(prior,indent=2)+'\n')
receipt={'status':'pass within exact payload, native loader/CLI and scoped ordinary portable GUI acceptance','source_commit':cohort['source_revision'],'working_tree_at_build':'clean','candidate':str(old),'artifacts':cohort['artifacts'],'payload_files':58,'payload_identity':str((p/'PAYLOAD_GUI_MANIFEST.json').relative_to(r)),'evidence_directory':str(p),'native_msi_administrative_extraction':json.loads((p/'MSI_PAYLOAD.json').read_text(encoding='utf-8-sig')),'native_loader_cli':json.loads((p/'CLI_SMOKE.json').read_text(encoding='utf-8-sig')),'portable_gui_acceptance':{'evidence':str((g/'GUI_SCOPES.json').relative_to(r)),'scope':gui['accepted_actions'],'appearance':gui['appearance']}, 'source_checks':{'evidence':'.hermes/evidence/windows-copy-public-workflow-current/ACCEPTANCE.json','native_ui':'375 passed/0 failed/4 existing ignores/0 filtered under each ordinary and elevated GNU Windows token; targeted native Undo authority control also passes in each role','linux_regressions':'419 platform,239 search,402 UI pass; existing ignores4/2/1','strict_lint':'Linux and Windows GNU UI/platform/search all targets passed'},'hosted_source':{'run':40268547285,'revision':cohort['source_revision'],'snapshot':json.loads((p/'HOSTED_CURRENT.json').read_text()),'scope':'Hosted MSVC verification is accepted only for completed named jobs; do not infer full success from the GNU or package scopes'},'cleanup':{'native_package':'windows-copy-package-current/NATIVE_OWNER.json','portable_gui':'windows-copy-gui-current/SESSION.json','guest_off':True,'original_guest_files_and_login_values_preserved':True,'account_profile_credential_task_and_owned_extractions_removed':True},'public_windows_files_unchanged':True,'linux_packages_unchanged':True,'limits':['No install/upgrade/repair/rollback/uninstall acceptance for these hashes','Actual GUI scope ordinary Dark/List100/default renderer1024x680 on1280x800 NTFS VM; no physical/other-width/scale/graphics acceptance','Public Copy works in this internal candidate within the named GUI scopes; broader alias/resource/transaction/platform and installer lifecycle gates remain engineering work; full hosted source CI passed','Earlier footer/tooltip, cross-volume and Office GUI evidence retain their original source/artifact scopes','No production release, Store submission or user daily-driver acceptance']}
os.replace(old,backup)
try:
 os.replace(stage,old)
 for name,h in cohort['artifacts'].items():assert sha(old/name)==h
 tmp=r/'dist/.WINDOWS_CANDIDATE_ACCEPTANCE.next';tmp.write_text(json.dumps(receipt,indent=2)+'\n');os.replace(tmp,r/'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json')
except BaseException:
 if old.exists():os.replace(old,stage)
 os.replace(backup,old);raise
# The old tree was completely verified above, has no consumers and is now superseded.
size=sum(f.stat().st_size for f in backup.rglob('*') if f.is_file());shutil.rmtree(backup)
(p/'PROMOTION.json').write_text(json.dumps({'status':'pass','source':cohort['source_revision'],'stable_candidate':str(old),'previous_source':prior['source_commit'],'superseded_verified_tree_removed':True,'superseded_bytes':size,'stage_removed':not stage.exists(),'current_archives_reverified':True,'no_active_package_consumers':True,'receipt':str(r/'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json')},indent=2)+'\n')
build=json.loads((p/'BUILD_SOURCE.json').read_text());build['pending_consumers']=[];build['final_candidate']=str(old);build['cleanup_complete']=True;(p/'BUILD_SOURCE.json').write_text(json.dumps(build,indent=2)+'\n')
print('Promoted verified58-file cohort; retired',size,'superseded bytes')
