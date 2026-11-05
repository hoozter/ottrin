"""Publish the verified current Windows candidate and retire its replaced output."""
from pathlib import Path
import argparse,hashlib,json,shutil,subprocess,zipfile
R=Path.cwd();E=Path(__file__).resolve().parent
read=lambda p:json.loads(p.read_text())
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def save(name,value):(E/name).write_text(json.dumps(value,indent=2)+'\n')
def unused(files):
 assert all(p.is_file() and not p.is_symlink() and p.stat().st_nlink==1 for p in files)
 for start in range(0,len(files),30):assert subprocess.run(['fuser',*map(str,files[start:start+30])],capture_output=True).returncode==1
parser=argparse.ArgumentParser();parser.add_argument('--publish',action='store_true');args=parser.parse_args()
stage=R/'.hermes/packaging-tmp/.windows-picker-stage';dest=R/'dist/windows-candidate';retired=R/'.hermes/packaging-tmp/.windows-candidate-retired';authority=R/'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json'
old_bytes=authority.read_bytes();assert old_bytes==(E/'PREVIOUS_WINDOWS_CANDIDATE_ACCEPTANCE.json').read_bytes();old=read(authority)
build=read(E/'windows/BUILD_RESULT.json');content=read(E/'HOST_PAYLOAD.json');assembly=read(E/'ASSEMBLY_ACCEPTANCE.json');native=read(E/'NATIVE_WINDOWS_UI_ACCEPTANCE.json');payload=read(E/'PAYLOAD.json');fresh=read(E/'MSIX_CONTEXTS_ACCEPTANCE.json');update=read(E/'MSIX_ENABLED_UPDATE_ACCEPTANCE.json')
source=build['source_revision'];assert source==content['source_revision']==assembly['source_revision']==native['source_revision']==fresh['source_revision']==update['source_revision']
assert all(v['status'].startswith('pass') for v in [content,assembly,native,fresh,update])
assert build['child_exit']==0 and len(payload)==59 and assembly['version']=='0.1.0.25'
assert content['portable_nsis_payloads_identical'] and content['owned_extraction_removed']
ci=read(E/'CI.json');assert ci['source_revision']==source and ci['status']=='completed' and ci['conclusion']=='success' and len(ci['jobs'])==4 and all(j['conclusion']=='success' for j in ci['jobs'])
for directory,receipt in [('native-nsis',native),('menus25',fresh),('update24to25',update)]:
 for name,digest in receipt['capture_review']['captures'].items():assert sha(E/directory/'results'/name)==digest
 assert receipt['cleanup']['domain_removed'] and receipt['cleanup']['fixture_removed'] and len(receipt['cleanup']['owned_volumes_removed'])==2
 assert not (E/directory/'RESULTS.zip').exists()
 owner=read(E/directory/'OWNER.json');assert not Path(owner['private_console']['home']).parent.exists()
 assert sha(E/directory/'DRIVER_SOURCE.py')==owner['producer_sha256']
assert len(native['capture_review']['captures'])==6
for receipt in [fresh,update]:
 assert len(receipt['capture_review']['captures'])==7 and receipt['package']['sha256']==assembly['artifact_sha256']
 assert receipt['removal']['package_removed'] and receipt['removal']['preferences_preserved'] and receipt['removal']['fixture_preserved']
 assert [c['context'] for c in receipt['contexts']]==['Background','Multiple','Drive']
 assert receipt['com_unregistration']['activation_hresult']==-2147221164
assert update['package']['previous']['version']=='0.1.0.24' and update['previous_folder_command']['enabled'] and update['previous_folder_command']['com_state']==0
assert read(E/'assembly25/OWNER.json')['cleanup']['domain_removed']
assert not subprocess.check_output(['virsh','-c','qemu:///system','list','--name'],text=True).strip(),'Finish owned native consumers before publication'
artifacts=content['artifacts']|{Path(assembly['artifact']).name:assembly['artifact_sha256']};assert len(artifacts)==3
for name,digest in artifacts.items():assert sha(stage/name)==digest and (stage/(name+'.sha256')).read_text().split()[0]==digest
for name,digest in payload.items():assert sha(stage/'windows-payload/Ottrin-0.1.0-windows-x64'/name)==digest
with zipfile.ZipFile(stage/'Ottrin-0.1.0-windows-x64-portable.zip') as z:
 zipped={str(Path(i.filename).relative_to('Ottrin-0.1.0-windows-x64')):hashlib.sha256(z.read(i)).hexdigest() for i in z.infolist() if not i.is_dir()}
assert zipped==payload
old_payload_root=dest/'windows-payload/Ottrin-0.1.0-windows-x64';old_payload={str(p.relative_to(old_payload_root)):sha(p) for p in old_payload_root.rglob('*') if p.is_file()};assert len(old_payload)==old['payload_files']==58
for name,digest in old['artifacts'].items():assert sha(dest/name)==digest
old_files=[p for p in dest.rglob('*') if p.is_file()]
def allowed(artifacts,payload):return {str(Path('windows-payload/Ottrin-0.1.0-windows-x64')/n) for n in payload}|set(artifacts)|{n+s for n in artifacts for s in ['.sha256','.provenance.json']}|{'PREVIEW_STATUS.md'}
assert {str(p.relative_to(dest)) for p in old_files}<=allowed(old['artifacts'],old_payload)
new_files=[p for p in stage.rglob('*') if p.is_file()];assert {str(p.relative_to(stage)) for p in new_files}<=allowed(artifacts,payload)
unused(old_files);unused(new_files)
held=R/'.hermes/evidence/windows-msi-package-control-current/input/DIAGNOSTIC_INPUT.json';held_bytes=held.read_bytes();held_input=read(held);assert sha(Path(held_input['artifact']))==held_input['sha256']
linux=R/'dist/packages/LINUX_PACKAGES_ACCEPTANCE.json';linux_bytes=linux.read_bytes();linux_input=read(linux)
# Windows-only schema/component changes do not require rebuilding delivered Linux.
assert linux_input['source_revision']=='bee15669e83353b043350a099b9041ccdd14e5c8'
assert not retired.exists() and not authority.with_suffix('.next').exists()
scope={'source_revision':source,'previous_delivered_source':old['source_commit'],'changed_product_files':subprocess.check_output(['git','diff','--name-only',old['source_commit'],source,'--','Cargo.toml','Cargo.lock','crates','packaging'],text=True).splitlines(),'current_nsis_receipt':'NATIVE_WINDOWS_UI_ACCEPTANCE.json','current_msix_receipts':['ASSEMBLY_ACCEPTANCE.json','MSIX_CONTEXTS_ACCEPTANCE.json','MSIX_ENABLED_UPDATE_ACCEPTANCE.json'],'basis':'Exact current59-file ZIP/NSIS equality, current ordinary-user NSIS Services, current fresh25 and enabled24-to25 native contexts/login/removal; all four exact-source hosted jobs pass. No historical package result substituted for current acceptance.'}
save('WINDOWS_SOURCE_SCOPE.json',scope)
save('WINDOWS_PROMOTION_PREFLIGHT.json',{'status':'ready for guarded Windows publication','source_revision':source,'artifacts':artifacts,'mutation_performed':False,'all_current_native_consumers_terminal':True,'scope':scope})
if not args.publish:print('Windows promotion preflight passed; no publication performed');raise SystemExit(0)
preview='''# Current Windows preview

Source SOURCE: verified59-file ZIP/NSIS and unsigned development MSIX25.
Actual installed Services Ready/Stopped/error/Ready pass in Dark and Light:
completed actions remain qualified neutral history, errors stay distinct,
recovery controls retain their geometry, complete diagnostics remain visible,
startup stays On and normal close keeps the daemon. Uninstall preserves files
and preferences. Six targeted Services originals were directly reviewed.

Current MSIX25 fresh runtime and actual enabled24-to25 upgrade pass folder
background, two selected folders and OS drive invocation, ordinary user/package
identity, automatic foreground, normal exit, real reboot/login before any
Ottrin CLI, preferences/index preservation and COM refusal after removal.
Seven originals per case were reviewed. The daemon has a separate native
private1000-file watcher/restart/shutdown check and30-second idle sample.

Default folder opening under MSIX still has an unresolved registry-transaction
error; context-menu acceptance does not close default-manager integration.
MSIX drive-menu schema requires Windows11 build22621; current native evidence
is from Windows11. Lower advertised package builds require their own integration
audit. Native light-menu icon contrast, supplementary glyph fallback, the full theme
property audit, administrator Copy and ordinary MSI rollback remain open.
No public release, Store, signing, physical GPU or human acceptance is implied.
See ../WINDOWS_CANDIDATE_ACCEPTANCE.json for exact evidence and limits.
'''.replace('SOURCE',source)
(stage/'PREVIEW_STATUS.md').write_text(preview)
manifest={'status':'pass scoped current Windows Services and packaged Explorer integration; full roadmap incomplete','source_commit':source,'working_tree_at_build':'clean','candidate':str(dest),'artifacts':artifacts,'payload_files':59,'evidence_directory':str(E.relative_to(R)),'hosted_source':{'run':42054602533,'tested_source_revision':source,'exact_source_all_four_jobs_success':True},'host_content':'HOST_PAYLOAD.json','current_native_fields':str((E/'NATIVE_WINDOWS_UI_ACCEPTANCE.json').relative_to(R)),'daemon_runtime':str((E/'WINDOWS_DAEMON_RUNTIME.json').relative_to(R)),'native_msix':{'current_assembly':str((E/'ASSEMBLY_ACCEPTANCE.json').relative_to(R)),'current_runtime_login_removal':str((E/'MSIX_CONTEXTS_ACCEPTANCE.json').relative_to(R)),'current_enabled_update':str((E/'MSIX_ENABLED_UPDATE_ACCEPTANCE.json').relative_to(R)),'scope':scope['basis']},'native_msi_rollback':old['native_msi_rollback'],'source_scope':str((E/'WINDOWS_SOURCE_SCOPE.json').relative_to(R)),'public_release_published':False,'limits':['Software guest/default DPI only; no physical GPU, signing, Store or human acceptance','Targeted Services and Explorer originals only; full theme/interface property audit remains open','MSIX default folder opening transaction error remains separate software work','MSIX drive-menu schema requires Windows11 build22621; lower advertised package builds are not accepted by this Windows11 case','Native light-menu icon contrast and supplementary glyph fallback remain open','Administrator Copy coordinator/caller registry and ordinary MSI rollback remain gated']}
save('WINDOWS_PROMOTION_INTENT.json',{'source_revision':source,'from':str(stage),'to':str(dest),'retired':str(retired),'all_current_native_consumers_terminal':True,'held_msi_preserved':held_input['sha256']})
swapped=False
try:
 dest.rename(retired)
 try:stage.rename(dest)
 except BaseException:retired.rename(dest);raise
 swapped=True
 for name,digest in artifacts.items():assert sha(dest/name)==digest
 for name,digest in payload.items():assert sha(dest/'windows-payload/Ottrin-0.1.0-windows-x64'/name)==digest
 assert held.read_bytes()==held_bytes and sha(Path(held_input['artifact']))==held_input['sha256'] and linux.read_bytes()==linux_bytes
 temp=authority.with_suffix('.next');temp.write_text(json.dumps(manifest,indent=2)+'\n');temp.replace(authority)
except BaseException:
 authority.with_suffix('.next').unlink(missing_ok=True)
 if swapped:dest.rename(stage);retired.rename(dest)
 authority.write_bytes(old_bytes)
 raise
removed=[{'path':str(p.relative_to(retired)),'bytes':p.stat().st_size,'sha256':sha(p)} for p in retired.rglob('*') if p.is_file()];unused([p for p in retired.rglob('*') if p.is_file()]);shutil.rmtree(retired)
assembly['assembled_artifact_path']=assembly['artifact'];assembly['artifact']=str(dest/Path(assembly['artifact']).name);save('ASSEMBLY_ACCEPTANCE.json',assembly)
save('WINDOWS_DELIVERY.json',{'status':'pass exact scoped Windows preview promoted','source_revision':source,'artifacts':artifacts,'payload_files':59,'superseded_files_removed':removed,'superseded_logical_bytes_removed':sum(v['bytes'] for v in removed),'stage_removed':not stage.exists(),'retired_removed':not retired.exists(),'held_msi_preserved':held_input['sha256'],'linux_authority_preserved':linux.read_bytes()==linux_bytes,'native_case_cleanup':{n:read(E/n/'OWNER.json')['cleanup'] for n in ['menus25','update24to25','native-nsis']}})
print('Verified Windows ZIP/NSIS/MSIX25 preview delivered; superseded artifacts and staging removed')
