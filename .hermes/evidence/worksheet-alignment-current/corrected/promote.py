"""Publish the verified worksheet cohort; preserve the held MSI and user data."""
from pathlib import Path
import argparse,hashlib,json,shutil,subprocess
r=Path.cwd();e=Path(__file__).resolve().parent
read=lambda p:json.loads(p.read_text())
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def unused(files):
 assert files and all(p.is_file() and not p.is_symlink() and p.stat().st_nlink==1 for p in files)
 assert subprocess.run(['fuser',*map(str,files)],capture_output=True).returncode==1
ci=read(e/'CI_CURRENT.json');source=read(e/'BUILD_SOURCE.json')['source_revision'];assert ci['status']=='completed' and ci['conclusion']=='success' and ci['headSha']==source
expected={'verify (ubuntu-latest)','verify (windows-latest)','search-performance (ubuntu-latest)','search-performance (windows-latest)'};assert {v['name'] for v in ci['jobs']}==expected and all(v['conclusion']=='success' for v in ci['jobs'])
assert read(e/'SOURCE_SCOPE.json')['candidate_source']==source
subprocess.run(['git','diff','--exit-code',source,'HEAD','--','.',':!.hermes/**',':!docs/ROADMAP.md',':!dist/**'],check=True,stdout=subprocess.PIPE)
subprocess.run(['git','diff','--exit-code','HEAD','--','.',':!.hermes/**',':!docs/ROADMAP.md',':!dist/**'],check=True,stdout=subprocess.PIPE)
for name in ['LINUX_NATIVE_ACCEPTANCE.json','WINDOWS_NATIVE_ACCEPTANCE.json']:
 a=read(e/name);assert a['source_revision']==source and a['status'].startswith('pass')
for case in ['upgrade-enabled-native-focus','upgrade-disabled-console']:
 a=read(e/case/'ACCEPTANCE.json');assert a['source_revision']==source and a['status'].startswith('pass');assert read(e/case/'NATIVE_VISIBILITY_ACCEPTANCE.json')['status']=='pass'
comparison=read(e/'FULL_LINUX_PAYLOAD_COMPARISON.json');assert comparison['status'].startswith('pass') and comparison['candidate_source']==source
comparison=read(e/'FULL_WINDOWS_PAYLOAD_COMPARISON.json');assert comparison['status'].startswith('pass') and comparison['candidate_source']==source
assert read(e/'WINDOWS_INDEX_PE_COMPARISON.json')['status'].startswith('pass: complete PE equal')
# Retire the exact hard-linked baseline only after both native consumers passed.
previous=read(e/'PREVIOUS_PACKAGE.json');alias=Path(previous['retained_input']);canonical=r/'dist/windows-candidate/Ottrin-0.1.0-windows-x64-development.msix'
assert alias.samefile(canonical) and alias.stat().st_nlink==canonical.stat().st_nlink==2 and sha(alias)==previous['sha256'];assert subprocess.run(['fuser',str(alias),str(canonical)],capture_output=True).returncode==1;alias.unlink();previous.update(pending_consumers=[],completed_consumers=['upgrade-enabled-native-focus','upgrade-disabled-console'],input_retired_after_both_consumers=True,canonical_previous=str(canonical),replay_limit='Exact former package consumed/verified before retirement; raw native input hashes and58-file previous manifest remain historical evidence');(e/'PREVIOUS_PACKAGE.json').write_text(json.dumps(previous,indent=2)+'\n')
# Linux: validate all candidate and previous bytes before the first publication.
linux=r/'dist/packages';stage=Path(read(e/'BUILD_SOURCE.json')['stage']);content=read(e/'PACKAGE_CONTENT_ACCEPTANCE.json');old=read(linux/'LINUX_PACKAGES_ACCEPTANCE.json')
assert content['source_revision']==source and content['status']=='pass'
for name,digest in old['artifacts'].items():assert sha(linux/name)==digest
for name,digest in content['artifacts'].items():assert sha(stage/name)==digest and (stage/(name+'.sha256')).read_text().split()[0]==digest
unused([p for p in stage.rglob('*') if p.is_file()]);(e/'PREVIOUS_LINUX_PACKAGES_ACCEPTANCE.json').write_bytes((linux/'LINUX_PACKAGES_ACCEPTANCE.json').read_bytes())
linux_receipt={'status':'pass exact current package/worksheet scopes; unchanged responsibilities carried by source and payload identity','source_revision':source,'artifacts':content['artifacts'],'evidence':str(e.relative_to(r)),'indexd_binary_sha256':read(e/'RUNTIME_IDENTITY.json')['payload']['usr/bin/ottrin-indexd']['candidate_sha256'],'checks':content['checks'],'current_native_worksheet_receipt':str((e/'LINUX_NATIVE_ACCEPTANCE.json').relative_to(r)),'source_scope':str((e/'SOURCE_SCOPE.json').relative_to(r)),'full_payload_comparison':str((e/'FULL_LINUX_PAYLOAD_COMPARISON.json').relative_to(r)),'unchanged_responsibility_basis':{'previous_source':old['source_revision'],'previous_receipt':str((e/'PREVIOUS_LINUX_PACKAGES_ACCEPTANCE.json').relative_to(r)),'scope':'Every runtime dependency/helper/asset/policy/launcher unchanged; shipping code differs only in independently verified worksheet row function. Previous integration/installer/resource/accessibility/workflow scopes remain historical, not relabelled as fresh runs.'},'hosted_source':{'run':41585803867,'tested_source_revision':source,'all_four_jobs_success':True},'limits':['Local delivery, no signed public release','Native X11/software Dark100/Light150; physical devices/GPU/final owner acceptance separate','Only worksheet GUI scope freshly rerun; broader unchanged scopes retain their original artifact hashes'],'cleanup':'All private GUI and native namespace fixtures/extractions removed; owned Linux staging retired after publication; current compiler caches retained.'}
for name in content['artifacts']:
 subprocess.run(['bash','-c','set -euo pipefail;source "$1";ottrin_publish_artifact "$2" "$3"','publish',str(r/'packaging/package-lifecycle.sh'),str(stage/name),str(linux/name)],check=True)
 for suffix in ['.sha256','.provenance.json']:
  side=stage/(name+suffix)
  if side.exists():shutil.move(side,linux/side.name)
 assert sha(linux/name)==content['artifacts'][name]
assert not any(stage.iterdir());stage.rmdir();(linux/'LINUX_PACKAGES_ACCEPTANCE.json').write_text(json.dumps(linux_receipt,indent=2)+'\n');(e/'LINUX_DELIVERY.json').write_text(json.dumps(linux_receipt,indent=2)+'\n')
b=read(e/'BUILD_SOURCE.json');b.update(pending_consumers=[],cleanup_complete=True,delivery_status='Verified4a Linux cohort promoted; staging removed');(e/'BUILD_SOURCE.json').write_text(json.dumps(b,indent=2)+'\n')
# Windows: swap the complete directory, then retire only verified prior output.
destination=r/'dist/windows-candidate';stage=Path(read(e/'windows/BUILD_SOURCE.json')['stage']);retired=r/'.hermes/packaging-tmp/.windows-candidate-retired';authority=r/'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json';assert not retired.exists()
old=read(authority);host=read(e/'HOST_PAYLOAD.json');assembly=read(e/'ASSEMBLY_ACCEPTANCE.json');payload=read(e/'PAYLOAD.json');artifacts=host['artifacts']|{Path(assembly['artifact']).name:assembly['artifact_sha256']}
assert host['source_revision']==assembly['source_revision']==source
for name,digest in old['artifacts'].items():assert sha(destination/name)==digest
old_payload={str(p.relative_to(destination/'windows-payload/Ottrin-0.1.0-windows-x64')):sha(p) for p in (destination/'windows-payload/Ottrin-0.1.0-windows-x64').rglob('*') if p.is_file()};assert len(old_payload)==58
old_files=[p for p in destination.rglob('*') if p.is_file()];known={str(Path('windows-payload/Ottrin-0.1.0-windows-x64')/n) for n in old_payload}|set(old['artifacts'])|{n+s for n in old['artifacts'] for s in ['.sha256','.provenance.json']}|{'PREVIEW_STATUS.md'};assert {str(p.relative_to(destination)) for p in old_files}<=known;unused(old_files)
for name,digest in artifacts.items():assert sha(stage/name)==digest
for name,digest in payload.items():assert sha(stage/'windows-payload/Ottrin-0.1.0-windows-x64'/name)==digest
held=r/'.hermes/evidence/windows-msi-package-control-current/input/DIAGNOSTIC_INPUT.json';held_bytes=held.read_bytes();diagnostic=read(held);assert sha(Path(diagnostic['artifact']))==diagnostic['sha256']
(stage/'PREVIEW_STATUS.md').write_text('# Current Windows preview\n\nSource `'+source+'`: ZIP/NSIS and unsigned development MSIX0.1.0.6.\n\nCurrent native installed worksheet typography and actual MSIX5-to6 update, ordinary GUI, preference/index identity, real login with startup enabled/disabled and removal pass. Every non-GUI payload is unchanged except daemon timestamp/checksum metadata; earlier broader scopes remain separately bound to original receipts.\n\nPhysical graphics/scaling, Store signing and final owner acceptance remain separate. Required MSI rollback remains held on its exact diagnostic input, not offered as a current package. See ../WINDOWS_CANDIDATE_ACCEPTANCE.json.\n')
new_files=[p for p in stage.rglob('*') if p.is_file()];allowed={str(Path('windows-payload/Ottrin-0.1.0-windows-x64')/n) for n in payload}|set(artifacts)|{n+s for n in artifacts for s in ['.sha256','.provenance.json']}|{'PREVIEW_STATUS.md'};assert {str(p.relative_to(stage)) for p in new_files}<=allowed;unused(new_files)
(e/'PREVIOUS_WINDOWS_CANDIDATE_ACCEPTANCE.json').write_bytes(authority.read_bytes());(e/'PROMOTION_INTENT.json').write_text(json.dumps({'source':source,'windows_from':str(stage),'windows_to':str(destination),'retired':str(retired),'all_native_consumers_terminal':True,'held_msi_sha256':diagnostic['sha256']},indent=2)+'\n')
destination.rename(retired)
try:stage.rename(destination)
except BaseException:retired.rename(destination);raise
for name,digest in artifacts.items():assert sha(destination/name)==digest
for name,digest in payload.items():assert sha(destination/'windows-payload/Ottrin-0.1.0-windows-x64'/name)==digest
assert held.read_bytes()==held_bytes and sha(Path(diagnostic['artifact']))==diagnostic['sha256']
receipt={'status':'pass current scoped local Windows preview; required MSI release held separately','source_commit':source,'working_tree_at_build':'clean','candidate':str(destination),'artifacts':artifacts,'payload_files':58,'evidence_directory':str(e.relative_to(r)),'host_content':'HOST_PAYLOAD.json','current_native_worksheet':str((e/'WINDOWS_NATIVE_ACCEPTANCE.json').relative_to(r)),'native_msix':{n:str((e/case/'ACCEPTANCE.json').relative_to(r)) for n,case in [('upgrade-enabled','upgrade-enabled-native-focus'),('upgrade-disabled','upgrade-disabled-console')]},'native_visibility':{n:str((e/case/'NATIVE_VISIBILITY_ACCEPTANCE.json').relative_to(r)) for n,case in [('upgrade-enabled','upgrade-enabled-native-focus'),('upgrade-disabled','upgrade-disabled-console')]},'source_scope':str((e/'SOURCE_SCOPE.json').relative_to(r)),'full_payload_comparison':str((e/'FULL_WINDOWS_PAYLOAD_COMPARISON.json').relative_to(r)),'daemon_pe_identity':str((e/'WINDOWS_INDEX_PE_COMPARISON.json').relative_to(r)),'unchanged_responsibility_basis':{'previous_source':old['source_commit'],'previous_receipt':str((e/'PREVIOUS_WINDOWS_CANDIDATE_ACCEPTANCE.json').relative_to(r)),'scope':'All prior installer-fault, conversion, resource and broader UI scopes retain original hashes. Current GUI row function checked independently; all other shipping source unchanged. Daemon entire PE identical except timestamp/checksum; other helpers/dependencies byte-identical.'},'native_msi_rollback':old['native_msi_rollback'],'hosted_source':{'run':41585803867,'tested_source_revision':source,'exact_source_all_four_jobs_success':True},'public_windows_files_unchanged':True,'limits':['Local unsigned preview, no official public release or Store submission','Software VM/default DPI; physical graphics, extended scaling and final owner review separate','Windows temporal PDF and explicit Save plus remaining roadmap audit still required'],'cleanup':'All native fixture VMs/media/consoles removed. Previous alias and superseded cohort retired after native consumers. Current stable payload and compact receipts retained.'}
temp=authority.with_suffix('.next');temp.write_text(json.dumps(receipt,indent=2)+'\n');temp.replace(authority)
removed=[{'path':str(p.relative_to(retired)),'bytes':p.stat().st_size,'sha256':sha(p)} for p in retired.rglob('*') if p.is_file()];unused([p for p in retired.rglob('*') if p.is_file()]);shutil.rmtree(retired)
(e/'WINDOWS_DELIVERY.json').write_text(json.dumps({'status':'pass current Windows preview promoted','source_revision':source,'published':str(destination),'artifacts':artifacts,'all58_payload_hashes_verified_after_promotion':True,'held_msi_preserved_sha256':diagnostic['sha256'],'removed_superseded_files':removed,'removed_superseded_logical_bytes':sum(v['bytes'] for v in removed),'staging_removed':not stage.exists(),'retired_directory_removed':not retired.exists(),'previous_input_removed':not alias.exists()},indent=2)+'\n')
b=read(e/'windows/BUILD_SOURCE.json');b.update(pending_consumers=[],cleanup_complete=True,delivery_status='Verified4a ZIP/NSIS/MSIX6 cohort promoted; staging removed');(e/'windows/BUILD_SOURCE.json').write_text(json.dumps(b,indent=2)+'\n')
print('Verified Linux four-format and Windows three-format cohorts promoted; superseded output removed.')
