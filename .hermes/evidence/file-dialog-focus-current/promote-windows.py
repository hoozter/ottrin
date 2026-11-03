"""Publish the exact accepted main-field cohort; preserve held MSI and user data."""
from pathlib import Path
import hashlib,json,shutil,subprocess
r=Path.cwd();e=Path(__file__).resolve().parent
read=lambda p:json.loads(p.read_text())
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def unused(files):
 assert files and all(p.is_file() and not p.is_symlink() and p.stat().st_nlink==1 for p in files)
 assert subprocess.run(['fuser',*map(str,files)],capture_output=True).returncode==1
source=read(e/'windows/BUILD_SOURCE.json')['source_revision'];ci=read(e/'CI_CURRENT.json')
assert ci['headSha']==source and ci['status']=='completed' and ci['conclusion']=='success'
assert {j['name'] for j in ci['jobs']}=={'verify (ubuntu-latest)','verify (windows-latest)','search-performance (ubuntu-latest)','search-performance (windows-latest)'} and all(j['conclusion']=='success' for j in ci['jobs'])
assert read(e/'SOURCE_SCOPE.json')['candidate_source']==source
subprocess.run(['git','diff','--exit-code',source,'--','.',':!.hermes/**',':!docs/ROADMAP.md',':!dist/**'],check=True,stdout=subprocess.PIPE)
assert read(e/'WINDOWS_NATIVE_ACCEPTANCE.json')['source_revision']==source
native=read(e/'WINDOWS_NATIVE_ACCEPTANCE.json')
assert native['source_revision']==source and native['status'].startswith('pass') and native['cases']==22
assert read(e/'FULL_WINDOWS_PAYLOAD_COMPARISON.json')['candidate_source']==source
assert read(e/'WINDOWS_INDEX_PE_COMPARISON.json')['status'].startswith('pass: complete PE equal')
assembly_owner=read(e/'assembly/OWNER.json');fields_owner=read(e/'windows-native-ui/OWNER.json')
for owner in [assembly_owner,fields_owner]:assert owner['cleanup']['domain_removed'] and owner['cleanup']['fixture_removed'] and len(owner['cleanup']['owned_volumes_removed'])==2
destination=r/'dist/windows-candidate';stage=Path(read(e/'windows/BUILD_SOURCE.json')['stage']);retired=r/'.hermes/packaging-tmp/.windows-candidate-retired';authority=r/'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json';assert not retired.exists()
old=read(authority);host=read(e/'HOST_PAYLOAD.json');assembly=read(e/'ASSEMBLY_ACCEPTANCE.json');payload=read(e/'PAYLOAD.json');artifacts=host['artifacts']|{Path(assembly['artifact']).name:assembly['artifact_sha256']}
assert host['source_revision']==assembly['source_revision']==source
for name,digest in old['artifacts'].items():assert sha(destination/name)==digest
old_payload={str(p.relative_to(destination/'windows-payload/Ottrin-0.1.0-windows-x64')):sha(p) for p in (destination/'windows-payload/Ottrin-0.1.0-windows-x64').rglob('*') if p.is_file()};assert len(old_payload)==58
old_files=[p for p in destination.rglob('*') if p.is_file()];known={str(Path('windows-payload/Ottrin-0.1.0-windows-x64')/n) for n in old_payload}|set(old['artifacts'])|{n+s for n in old['artifacts'] for s in ['.sha256','.provenance.json']}|{'PREVIEW_STATUS.md'};assert {str(p.relative_to(destination)) for p in old_files}<=known;unused(old_files)
for name,digest in artifacts.items():assert sha(stage/name)==digest
for name,digest in payload.items():assert sha(stage/'windows-payload/Ottrin-0.1.0-windows-x64'/name)==digest
held=r/'.hermes/evidence/windows-msi-package-control-current/input/DIAGNOSTIC_INPUT.json';held_bytes=held.read_bytes();diagnostic=read(held);assert sha(Path(diagnostic['artifact']))==diagnostic['sha256']
(stage/'PREVIEW_STATUS.md').write_text('# Current Windows preview\n\nSource `'+source+'`: ZIP/NSIS and unsigned development MSIX 0.1.0.10.\n\nCurrent installed native Dark/Light Settings radio choice names/selection/actions and matching creation/rename input-height, typing/cancellation and Cancel-to-F2 focus checks pass; source bytes and preferences survive supported removal. Development MSIX revision 10 passes native MakeAppx assembly/unpack and complete 58-file equality with this ZIP/NSIS payload. Runtime/update/login acceptance is historical MSIX revision 8, linked with its exact original hashes; no fresh 8-to-10 update/login claim. All non-GUI helpers and libraries match; the complete daemon differs only in timestamp/checksum.\n\nPrivate software VM/default DPI; physical graphics, Store submission, human assistive usability and full roadmap acceptance remain separate. Required MSI rollback remains held. See ../WINDOWS_CANDIDATE_ACCEPTANCE.json.\n')

new_files=[p for p in stage.rglob('*') if p.is_file()];allowed={str(Path('windows-payload/Ottrin-0.1.0-windows-x64')/n) for n in payload}|set(artifacts)|{n+s for n in artifacts for s in ['.sha256','.provenance.json']}|{'PREVIEW_STATUS.md'};assert {str(p.relative_to(stage)) for p in new_files}<=allowed;unused(new_files)
assert (e/'PREVIOUS_WINDOWS_CANDIDATE_ACCEPTANCE.json').read_bytes()==authority.read_bytes();(e/'PROMOTION_INTENT.json').write_text(json.dumps({'source':source,'windows_from':str(stage),'windows_to':str(destination),'retired':str(retired),'all_native_consumers_terminal':True,'held_msi_sha256':diagnostic['sha256']},indent=2)+'\n')
destination.rename(retired)
try:stage.rename(destination)
except BaseException:retired.rename(destination);raise
for name,digest in artifacts.items():assert sha(destination/name)==digest
for name,digest in payload.items():assert sha(destination/'windows-payload/Ottrin-0.1.0-windows-x64'/name)==digest
assert held.read_bytes()==held_bytes and sha(Path(diagnostic['artifact']))==diagnostic['sha256']
receipt={'status':'pass current scoped local Windows navigation/filename geometry preview; required MSI release held separately','source_commit':source,'working_tree_at_build':'clean','candidate':str(destination),'artifacts':artifacts,'payload_files':58,'evidence_directory':str(e.relative_to(r)),'host_content':'HOST_PAYLOAD.json','current_native_fields':str((e/'WINDOWS_NATIVE_ACCEPTANCE.json').relative_to(r)),'native_msix':{'current_assembly':str((e/'ASSEMBLY_ACCEPTANCE.json').relative_to(r)),'historical_lifecycle':old['native_msix'],'historical_source':old['source_commit'],'limit':'No fresh MSIX revision 10 deployment/update/login/removal run; earlier MSIX revision 8 runtime gate keeps exact source/artifact scope. Shipping installation/startup/daemon source unchanged, all 58 assembled payload bytes match current native-checked NSIS/ZIP.'},'source_scope':str((e/'SOURCE_SCOPE.json').relative_to(r)),'full_payload_comparison':str((e/'FULL_WINDOWS_PAYLOAD_COMPARISON.json').relative_to(r)),'daemon_pe_identity':str((e/'WINDOWS_INDEX_PE_COMPARISON.json').relative_to(r)),'unchanged_responsibility_basis':{'previous_source':old['source_commit'],'previous_receipt':str((e/'PREVIOUS_WINDOWS_CANDIDATE_ACCEPTANCE.json').relative_to(r)),'scope':'Only Settings navigation widget metadata, centred shared Create/Rename field geometry, and release of the closed filename dialog input focus, plus associated tests, change shipping source. Earlier lifecycle, picker, installer-fault, Office/media, resource and broader UI scopes retain original hashes. Every other non-GUI payload matches; daemon PE differs only in timestamp/checksum.'},'native_msi_rollback':old['native_msi_rollback'],'hosted_source':{'run':41672693221,'tested_source_revision':source,'exact_source_all_four_jobs_success':True},'public_release_published':False,'limits':['Local unsigned preview; no public release or Store submission','Software VM/default DPI; no physical graphics/human assistive or final owner acceptance','Whole roadmap and numeric/color-picker accessibility remain unaccepted','Current main-field native scope and MSIX revision 10 assembly only; historical lifecycle scopes are explicitly linked'],'cleanup':'Both native VMs/media/private console removed; stage and superseded candidate retired after consumers; current58-file payload and compact evidence retained.'}

temp=authority.with_suffix('.next');temp.write_text(json.dumps(receipt,indent=2)+'\n');temp.replace(authority)
removed=[{'path':str(p.relative_to(retired)),'bytes':p.stat().st_size,'sha256':sha(p)} for p in retired.rglob('*') if p.is_file()];unused([p for p in retired.rglob('*') if p.is_file()]);shutil.rmtree(retired)
assembly['assembled_artifact_path']=assembly['artifact'];assembly['artifact']=str(destination/Path(assembly['artifact']).name);(e/'ASSEMBLY_ACCEPTANCE.json').write_text(json.dumps(assembly,indent=2)+'\n')
(e/'WINDOWS_DELIVERY.json').write_text(json.dumps({'status':'pass current Windows preview promoted','source_revision':source,'published':str(destination),'artifacts':artifacts,'all 58_payload_hashes_verified_after_promotion':True,'held_msi_preserved_sha256':diagnostic['sha256'],'removed_superseded_files':removed,'removed_superseded_logical_bytes':sum(v['bytes'] for v in removed),'staging_removed':not stage.exists(),'retired_directory_removed':not retired.exists()},indent=2)+'\n')
b=read(e/'windows/BUILD_SOURCE.json');b.update(pending_consumers=[],cleanup_complete=True,delivery_status='Verified navigation/filename geometry ZIP/NSIS/MSIX revision 10 cohort promoted; staging removed');(e/'windows/BUILD_SOURCE.json').write_text(json.dumps(b,indent=2)+'\n')
print('Verified Windows ZIP/NSIS/MSIX revision 10 cohort promoted; exact superseded output retired.')
