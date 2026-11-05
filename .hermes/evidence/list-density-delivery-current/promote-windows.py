"""Publish the exact verified Windows preview and retire superseded output."""
from pathlib import Path
import argparse,hashlib,json,shutil,subprocess,zipfile
R=Path.cwd();E=Path(__file__).resolve().parent
read=lambda p:json.loads(p.read_text())
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def save(name,value):(E/name).write_text(json.dumps(value,indent=2)+'\n')
def unused(files):
 assert all(p.is_file() and not p.is_symlink() and p.stat().st_nlink==1 for p in files)
 for start in range(0,len(files),30):
  assert subprocess.run(['fuser',*map(str,files[start:start+30])],capture_output=True).returncode==1
parser=argparse.ArgumentParser();parser.add_argument('--publish',action='store_true');args=parser.parse_args()
stage=R/'.hermes/packaging-tmp/.windows-picker-stage';dest=R/'dist/windows-candidate';retired=R/'.hermes/packaging-tmp/.windows-candidate-retired';authority=R/'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json'
old_bytes=authority.read_bytes();assert old_bytes==(E/'PREVIOUS_WINDOWS_CANDIDATE_ACCEPTANCE.json').read_bytes();old=read(authority)
build=read(E/'windows/BUILD_RESULT.json');content=read(E/'HOST_PAYLOAD.json');assembly=read(E/'ASSEMBLY_ACCEPTANCE.json');native=read(E/'NATIVE_WINDOWS_CAPTION_ACCEPTANCE.json');runtime=read(E/'MSIX_RUNTIME_ACCEPTANCE.json');payload=read(E/'PAYLOAD.json')
source=build['source_revision'];assert source==content['source_revision']==assembly['source_revision']==native['source_revision']==runtime['source_revision']
assert all(v['status'].startswith('pass') for v in [content,assembly,native,runtime])
assert len(payload)==58 and assembly['version']=='0.1.0.19'
ci=read(E/'CI_CURRENT.json');assert ci['headSha']==source and ci['status']=='completed' and ci['conclusion']=='success' and len(ci['jobs'])==4 and all(j['conclusion']=='success' for j in ci['jobs'])
for name,digest in native['capture_review']['captures'].items():assert sha(E/'native-captions/results'/name)==digest
for name,digest in runtime['capture_review']['captures'].items():assert sha(E/'runtime19/results'/name)==digest
assert runtime['capture_review']['settings_visible']
assert read(E/'runtime19/OWNER.json')['cleanup']['domain_removed']
assert not (E/'runtime19/RESULTS.zip').exists() and not (E/'native-captions/RESULTS.zip').exists()
prior=E.parent/'status-error-delivery-current';updates=read(prior/'MSIX_UPGRADE_ACCEPTANCE.json');prior_source=updates['source_revision']
assert updates['status'].startswith('pass')
delta=subprocess.check_output(['git','diff','--name-only',prior_source,source,'--','crates','packaging/windows'],text=True).splitlines()
assert delta==['crates/ottrin-ui/src/lib.rs']
scope={'source_revision':source,'previous_native_update_source':prior_source,'changed_product_files_since_native_update':delta,'update_basis':'Actual enabled/disabled17-to18 updates retain exact previous source/artifacts. Only UI renderer source changed since those tests; current19 has exact assembly plus fresh ordinary runtime/reboot/login/removal, without claiming a fresh17-to19 update.','previous_update_receipt':str((prior/'MSIX_UPGRADE_ACCEPTANCE.json').relative_to(R))}
save('WINDOWS_SOURCE_SCOPE.json',scope)
artifacts=content['artifacts']|{Path(assembly['artifact']).name:assembly['artifact_sha256']};assert len(artifacts)==3
for name,digest in artifacts.items():
 assert sha(stage/name)==digest and (stage/(name+'.sha256')).read_text().split()[0]==digest
for name,digest in payload.items():assert sha(stage/'windows-payload/Ottrin-0.1.0-windows-x64'/name)==digest
with zipfile.ZipFile(stage/'Ottrin-0.1.0-windows-x64-portable.zip') as z:
 zipped={str(Path(i.filename).relative_to('Ottrin-0.1.0-windows-x64')):hashlib.sha256(z.read(i)).hexdigest() for i in z.infolist() if not i.is_dir()}
assert zipped==payload
old_payload_root=dest/'windows-payload/Ottrin-0.1.0-windows-x64';old_payload={str(p.relative_to(old_payload_root)):sha(p) for p in old_payload_root.rglob('*') if p.is_file()};assert len(old_payload)==58
for name,digest in old['artifacts'].items():assert sha(dest/name)==digest
old_files=[p for p in dest.rglob('*') if p.is_file()]
def allowed(artifacts,payload):return {str(Path('windows-payload/Ottrin-0.1.0-windows-x64')/n) for n in payload}|set(artifacts)|{n+s for n in artifacts for s in ['.sha256','.provenance.json']}|{'PREVIEW_STATUS.md'}
assert {str(p.relative_to(dest)) for p in old_files}<=allowed(old['artifacts'],old_payload)
new_files=[p for p in stage.rglob('*') if p.is_file()];assert {str(p.relative_to(stage)) for p in new_files}<=allowed(artifacts,payload)
unused(old_files);unused(new_files)
held=R/'.hermes/evidence/windows-msi-package-control-current/input/DIAGNOSTIC_INPUT.json';held_bytes=held.read_bytes();held_input=read(held);assert sha(Path(held_input['artifact']))==held_input['sha256']
linux=R/'dist/packages/LINUX_PACKAGES_ACCEPTANCE.json';linux_bytes=linux.read_bytes();assert read(linux)['source_revision']==source
assert not retired.exists() and not authority.with_suffix('.next').exists()
save('WINDOWS_PROMOTION_PREFLIGHT.json',{'status':'ready for guarded Windows publication','source_revision':source,'artifacts':artifacts,'mutation_performed':False,'all_current_native_consumers_terminal':True,'historical_update_scope':scope})
if not args.publish:print('Windows promotion preflight passed; no publication performed');raise SystemExit(0)
preview='''# Current Windows preview

Source `'''+source+'''`: verified ZIP/NSIS and unsigned development MSIX19.
Native filename containment, own-density75/100/185, default-view persistence,
actual startup controls and ordinary uninstall preservation pass. MSIX19 has
fresh ordinary runtime, visible startup Settings, actual reboot/login and
removal acceptance. Earlier enabled/disabled17-to18 update evidence retains
its original source/artifact scope; no fresh17-to19 update is claimed.

Full theme/role audit remains open, including high-density icon/text padding
and Light contrast. MSIX folder integration reports unavailable transaction
support and remains an explicit software investigation. Administrator Copy
recovery is gated. Ordinary-user MSI rollback remains held separately.
No public release, Store/signing, physical GPU or final human acceptance.
See ../WINDOWS_CANDIDATE_ACCEPTANCE.json for exact hashes and evidence.
'''
(stage/'PREVIEW_STATUS.md').write_text(preview)
manifest={'status':'pass scoped native Windows filename/density/Services preview; full roadmap incomplete','source_commit':source,'working_tree_at_build':'clean','candidate':str(dest),'artifacts':artifacts,'payload_files':58,'evidence_directory':str(E.relative_to(R)),'hosted_source':{'run':ci['run_id'],'tested_source_revision':source,'exact_source_all_four_jobs_success':True},'host_content':'HOST_PAYLOAD.json','current_native_fields':str((E/'NATIVE_WINDOWS_CAPTION_ACCEPTANCE.json').relative_to(R)),'daemon_runtime':str((E/'WINDOWS_DAEMON_RUNTIME.json').relative_to(R)),'native_msix':{'current_assembly':str((E/'ASSEMBLY_ACCEPTANCE.json').relative_to(R)),'current_runtime_login_removal':str((E/'MSIX_RUNTIME_ACCEPTANCE.json').relative_to(R)),'historical_enabled_disabled_updates':scope['previous_update_receipt'],'scope':scope['update_basis']},'native_msi_rollback':old['native_msi_rollback'],'source_scope':str((E/'WINDOWS_SOURCE_SCOPE.json').relative_to(R)),'public_release_published':False,'limits':['Software guest/default DPI only; no physical graphics, signing, Store or final human acceptance','Full UI/theme audit open: high-density icon/text padding and Light footer/logo/status contrast','Fresh19 Settings shows unavailable folder-integration transaction support; independent software investigation remains open','Administrator Copy durable coordinator/caller registry remains gated; focused metadata preparation tests are not full acceptance','Fresh19 lifecycle and previous17-to18 update scopes are distinct; no17-to19 update claim']}
save('WINDOWS_PROMOTION_INTENT.json',{'source_revision':source,'from':str(stage),'to':str(dest),'retired':str(retired),'all_current_native_consumers_terminal':True,'previous17_update_input_consumer_closed':'No redundant17-to19 matrix: update implementation unchanged from fully native-tested7fa; exact older update proof retained with original scope','held_msi_preserved':held_input['sha256']})
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
save('WINDOWS_DELIVERY.json',{'status':'pass exact scoped Windows preview promoted','source_revision':source,'artifacts':artifacts,'payload_files':58,'superseded_files_removed':removed,'superseded_logical_bytes_removed':sum(v['bytes'] for v in removed),'stage_removed':not stage.exists(),'retired_removed':not retired.exists(),'held_msi_preserved':held_input['sha256'],'linux_authority_preserved':linux.read_bytes()==linux_bytes,'runtime_fixture_removed':read(E/'runtime19/OWNER.json')['cleanup']})
print('Verified Windows ZIP/NSIS/MSIX19 preview delivered; superseded artifacts and staging removed')
