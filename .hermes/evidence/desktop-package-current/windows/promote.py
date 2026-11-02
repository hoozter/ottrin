"""Promote one exact verified internal Windows cohort after Linux publication and native teardown."""
from pathlib import Path
import hashlib,json,os,subprocess,shutil,zipfile
r=Path.cwd();p=Path(__file__).resolve().parent;stage=Path(json.loads((p/'BUILD_SOURCE.json').read_text())['stage']);target=r/'dist/windows-candidate';backup=target.with_name('.windows-desktop-superseded')
def read(f):return json.loads(Path(f).read_text(encoding='utf-8-sig'))
def sha(f):
 with Path(f).open('rb') as h:return hashlib.file_digest(h,'sha256').hexdigest()
cohort=read(p/'HOST_PAYLOAD.json');source=cohort['source_revision'];hosted=read(p.parent/'HOSTED_SOURCE.json');owner=read(p/'gui/RUNTIME_OWNER.json');gui=read(p/'gui/short-temp-ordinary-folder-desktop.json')
assert subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()==source
changed=set(subprocess.check_output(['git','diff','--name-only','HEAD'],text=True).splitlines());assert changed<={'dist/packages/LINUX_PACKAGES_ACCEPTANCE.json'},changed
assert hosted['headSha']==source and hosted['conclusion']=='success'
assert read(p/'NATIVE_OWNER.json')['status']=='pass' and owner['status']=='pass' and owner['guest_off'] and owner['root_removed']
assert gui['status']=='pass' and gui['uninstallRestoredIntegration'] and gui['originalDefaultsPreserved'] and gui['sourcePreserved']
assert len(gui['windows'])==2 and all(w['exit']==0 for w in gui['windows'])
assert len(gui['picker'])==2 and all(a['exit']==0 and a['mainConfigUnchanged'] and a['sourceUnchanged'] and a['indexProcessesUnchanged'] for a in gui['picker'])
assert read(p/'EXECUTABLE_ICONS.json')['status']=='pass'
assert read(p/'VISUAL_REVIEW.json')['status']=='reviewed'
assert subprocess.check_output(['virsh','-c','qemu:///system','domstate','ottrin-copy-cleanup-qa'],text=True).strip()=='shut off'
linux=read(r/'dist/packages/LINUX_PACKAGES_ACCEPTANCE.json');assert linux['source_revision']==source and linux==read(p.parent/'linux/ACCEPTANCE.json')
for name,h in linux['artifacts'].items():assert sha(r/'dist/packages'/name)==h
prior=read(r/'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json');assert Path(prior['candidate'])==target and not backup.exists()
for name,h in read(p/'DELIVERABLE_BASELINE.json').items():assert sha(r/name)==h
for folder,hashes in [(target,prior['artifacts']),(stage,cohort['artifacts'])]:
 assert {f.name for f in folder.iterdir()}==set(hashes)|{n+'.sha256' for n in hashes}|{'windows-payload'}
 for name,h in hashes.items():assert sha(folder/name)==h and (folder/(name+'.sha256')).read_text().split()==[h,name]
 with zipfile.ZipFile(folder/'Ottrin-0.1.0-windows-x64-portable.zip') as z:
  expected={f.filename:hashlib.sha256(z.read(f)).hexdigest() for f in z.infolist() if not f.is_dir()}
 assert expected=={str(f.relative_to(folder/'windows-payload')):sha(f) for f in (folder/'windows-payload').rglob('*') if f.is_file()}
 files=[f for f in folder.rglob('*') if f.is_file()];assert not any(f.is_symlink() for f in folder.rglob('*'));assert all(f.stat().st_nlink==1 for f in files);assert subprocess.run(['fuser',*map(str,files)],capture_output=True).returncode==1
receipt={'status':'pass within exact payload,native loader/icons,ordinary folder/default/Properties/picker GUI and hosted source scopes','source_commit':source,'working_tree_at_build':'clean','candidate':str(target),'artifacts':cohort['artifacts'],'payload_files':58,'evidence_directory':str(p),'native_msi_administrative_extraction':read(p/'MSI_PAYLOAD.json'),'native_loader_cli':read(p/'CLI_SMOKE.json'),'native_icons':'EXECUTABLE_ICONS.json','ordinary_gui':'gui/short-temp-ordinary-folder-desktop.json','gui_scope':'Exact packaged app/index/helper/Mesa bytes in an owned flat native fixture; real default directory/drive launches,Settings enable/restore,long Properties/native Close,Save/cancel/no-write/config preservation and unchanged index process set; original typed defaults restored through uninstall. Isolated100percent ordinary OpenGL VM,not physical GPU or installed-product lifecycle.','hosted_source':{'run':hosted['databaseId'],'revision':source,'conclusion':'success'},'cleanup':{'assembly':'NATIVE_OWNER.json','gui':'gui/RUNTIME_OWNER.json','guest_off':True,'owned_roots_removed':True,'original_profiles_and_files_preserved':True},'public_windows_files_unchanged':True,'linux_source_cohort':source,'limits':['No new MSI install/upgrade/repair/rollback acceptance for these hashes; MSI rollback remains required and failed','GUI fixture uses exact binaries but does not establish full installed application runtime placement or physical/DPI acceptance','Prior Office,cross-volume,file-operation and sustained resource receipts retain their exact hashes','No public release or Store submission']}
(p/'PREVIOUS_ACCEPTANCE.json').write_text(json.dumps(prior,indent=2)+'\n')
os.replace(target,backup)
try:
 os.replace(stage,target)
 for name,h in cohort['artifacts'].items():assert sha(target/name)==h
 temporary=r/'dist/.WINDOWS_CANDIDATE_ACCEPTANCE.next';temporary.write_text(json.dumps(receipt,indent=2)+'\n');os.replace(temporary,r/'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json')
except BaseException:
 if target.exists():os.replace(target,stage)
 os.replace(backup,target);raise
size=sum(f.stat().st_size for f in backup.rglob('*') if f.is_file());shutil.rmtree(backup)
build=read(p/'BUILD_SOURCE.json');build.update(pending_consumers=[],cleanup_complete=True,final_candidate=str(target));(p/'BUILD_SOURCE.json').write_text(json.dumps(build,indent=2)+'\n')
(p/'PROMOTION.json').write_text(json.dumps({'status':'pass','source':source,'stable_candidate':str(target),'superseded_bytes_removed':size,'stage_removed':not stage.exists(),'superseded_directory_removed':not backup.exists()},indent=2)+'\n')
print('Promoted exact internal Windows cohort; superseded staging removed.')
