"""Promote one accepted Windows cohort; preserve the separately held failed MSI."""
from pathlib import Path
import hashlib,json,shutil,subprocess,zipfile
r=Path.cwd();e=Path(__file__).resolve().parent;stage=r/'.hermes/packaging-tmp/.windows-picker-stage';destination=r/'dist/windows-candidate';retired=r/'.hermes/packaging-tmp/.windows-candidate-retired';authority=r/'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json'
read=lambda p:json.loads(p.read_text())
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def unused(files):
 assert all(not p.is_symlink() and p.stat().st_nlink==1 for p in files)
 assert subprocess.run(['fuser',*map(str,files)],capture_output=True).returncode==1
assert not (e/'DELIVERY.json').exists() and not retired.exists()
old=read(authority);host=read(e/'HOST_PAYLOAD.json');assembly=read(e/'ASSEMBLY_ACCEPTANCE.json');nsis=read(e/'nsis-native/ACCEPTANCE.json');uia=read(e/'WINDOWS_UIA_ACCEPTANCE.json');payload=read(e/'PAYLOAD.json');previous=read(e/'PREVIOUS_PACKAGE.json')
source=host['source_revision'];assert source=='90ba8639f2e5d6dad7e24ebd5b95382e108e1300'
assert old['source_commit']=='656768852a930e4e69dcf37e901d5bf00c6b6175'
assert assembly['source_revision']==nsis['source_revision']==uia['source_revision']==source and assembly['version']=='0.1.0.5'
assert nsis['status'].startswith('pass within exact ordinary-user') and uia['status'].startswith('pass within current complete')
assert read(e/'nsis-native/VISUAL_REVIEW.json')['status']=='reviewed'
for name in ['upgrade-enabled','upgrade-disabled']:
 case=e/name;accepted=read(case/'ACCEPTANCE.json');visibility=read(case/'NATIVE_VISIBILITY_ACCEPTANCE.json')
 assert accepted['status'].startswith('pass within actual native') and accepted['source_revision']==visibility['source_revision']==source and visibility['status']=='pass'
 assert accepted['package_input']['startup_enabled'] is (name=='upgrade-enabled')
 assert accepted['package_input']['sha256']==assembly['artifact_sha256'] and accepted['package_input']['previous']['sha256']==previous['sha256']
 assert read(case/'OWNER.json')['cleanup']['domain_removed']
ci=read(e/'HOSTED_FINAL.json');assert ci['headSha']==source and ci['conclusion']=='success' and len(ci['jobs'])==4 and all(j['conclusion']=='success' for j in ci['jobs'])
subprocess.run(['git','diff','--exit-code',source,'HEAD','--','Cargo.toml','Cargo.lock','crates','assets','packaging','scripts','build-all.sh'],check=True,stdout=subprocess.DEVNULL)
held=r/old['native_msi_rollback']['diagnostic_input'];held_bytes=held.read_bytes();diagnostic=read(held);assert sha(Path(diagnostic['artifact']))==diagnostic['sha256']
artifacts=host['artifacts']|{Path(assembly['artifact']).name:assembly['artifact_sha256']}
assert len(payload)==58
for name,digest in artifacts.items():assert sha(stage/name)==digest
for name,digest in payload.items():assert sha(stage/'windows-payload/Ottrin-0.1.0-windows-x64'/name)==digest
with zipfile.ZipFile(stage/'Ottrin-0.1.0-windows-x64-portable.zip') as z:
 zipped={str(Path(x.filename).relative_to('Ottrin-0.1.0-windows-x64')):hashlib.sha256(z.read(x)).hexdigest() for x in z.infolist() if not x.is_dir()}
assert zipped==payload
old_payload_root=destination/'windows-payload/Ottrin-0.1.0-windows-x64';old_payload={str(p.relative_to(old_payload_root)):sha(p) for p in old_payload_root.rglob('*') if p.is_file()};assert len(old_payload)==58
for name,digest in old['artifacts'].items():assert sha(destination/name)==digest
old_files=[p for p in destination.rglob('*') if p.is_file()];known={str(Path('windows-payload/Ottrin-0.1.0-windows-x64')/name) for name in old_payload}|set(old['artifacts'])|{name+'.sha256' for name in old['artifacts']}|{name+'.provenance.json' for name in old['artifacts']}|{'PREVIEW_STATUS.md'}
assert {str(p.relative_to(destination)) for p in old_files}<=known,'Unexpected previous candidate files retained'
# Both native update consumers are terminal; retire only the exact alias.
alias=Path(previous['retained_input']);canonical=Path(previous['canonical_previous']);assert alias==stage/'previous-development.msix' and canonical.parent==destination
assert alias.samefile(canonical) and sha(alias)==previous['sha256'] and alias.stat().st_nlink==canonical.stat().st_nlink==2
assert subprocess.run(['fuser',str(alias),str(canonical)],capture_output=True).returncode==1
alias.unlink();unused(old_files)
previous.update(pending_consumers=[],input_retired_after_both_consumers=True,replay_limit='Exact old/current packages checked by both native assessors before promotion; former retained_input path is now historical')
(e/'PREVIOUS_PACKAGE.json').write_text(json.dumps(previous,indent=2)+'\n')
provenance=read(e/'assembly/ASSEMBLY.json')['provenance'];assert provenance['version']=='0.1.0.5' and not provenance['signed'] and provenance['sha256']==assembly['artifact_sha256']
(stage/(Path(assembly['artifact']).name+'.provenance.json')).write_text(json.dumps(provenance,indent=2)+'\n')
for name,digest in artifacts.items():(stage/(name+'.sha256')).write_text(digest+'  '+name+'\n')
(stage/'PREVIEW_STATUS.md').write_text('''# Current Windows preview

Verified portable ZIP, ordinary-user NSIS installer and development MSIX from
source `'''+source+'''`.

Current portable WGPU/OpenGL UIAutomation, read-only Inspector, chooser and
populated-index observations pass. Native NSIS fresh installation/removal,
upgrade, saved Miller view, startup/running intent and real cleanup refusal/retry
pass. MSIX4→5 updates preserve preferences and database identity; real reboot/login
enabled and disabled startup choices and package removal pass.

The unsigned development MSIX uses the Ottrin Development identity and explicit
administrative Add-AppxPackage -AllowUnsigned deployment in owned QA. It is not a
Store-signed distribution. ZIP/NSIS remain the ordinary-user preview channels.

Physical GPUs, extended DPI, screen-reader usability, broader roadmap acceptance
and final owner acceptance remain separate. The required MSI remains held on the
known ordinary-user Windows Installer rollback boundary; the failed exact older
input is retained separately and is not offered here as a current usable package.
See ../WINDOWS_CANDIDATE_ACCEPTANCE.json.
''')
allowed={str(Path('windows-payload/Ottrin-0.1.0-windows-x64')/name) for name in payload}|set(artifacts)|{name+'.sha256' for name in artifacts}|{name+'.provenance.json' for name in artifacts}|{'PREVIEW_STATUS.md'}
new_files=[p for p in stage.rglob('*') if p.is_file()];assert {str(p.relative_to(stage)) for p in new_files}<=allowed;unused(new_files)
(e/'PREVIOUS_WINDOWS_CANDIDATE_ACCEPTANCE.json').write_bytes(authority.read_bytes())
(e/'PROMOTION_INTENT.json').write_text(json.dumps({'status':'Exact package/native/cleanup gates verified before directory moves','source_revision':source,'from':str(stage),'to':str(destination),'retired':str(retired),'previous_alias_removed_after_both_consumers':True,'held_msi_sha256':diagnostic['sha256']},indent=2)+'\n')
destination.rename(retired)
try:stage.rename(destination)
except BaseException:retired.rename(destination);raise
for name,digest in artifacts.items():assert sha(destination/name)==digest
for name,digest in payload.items():assert sha(destination/'windows-payload/Ottrin-0.1.0-windows-x64'/name)==digest
assert held.read_bytes()==held_bytes and sha(Path(diagnostic['artifact']))==diagnostic['sha256']
current={'status':'pass within recorded local Windows preview scopes; required MSI release held separately','source_commit':source,'working_tree_at_build':'clean','candidate':str(destination),'artifacts':artifacts,'payload_files':58,'evidence_directory':str(e.relative_to(r)),'host_content':'HOST_PAYLOAD.json','ordinary_nsis_and_portable':str((e/'nsis-native/ACCEPTANCE.json').relative_to(r)),'rendered_review':str((e/'nsis-native/VISUAL_REVIEW.json').relative_to(r)),'native_msix':{name:str((e/name/'ACCEPTANCE.json').relative_to(r)) for name in ['upgrade-enabled','upgrade-disabled']},'native_visibility':{name:str((e/name/'NATIVE_VISIBILITY_ACCEPTANCE.json').relative_to(r)) for name in ['upgrade-enabled','upgrade-disabled']},'native_accessibility':str((e/'WINDOWS_UIA_ACCEPTANCE.json').relative_to(r)),'native_msi_rollback':old['native_msi_rollback'],'hosted_source':{'run':41565016646,'tested_source_revision':source,'exact_source_all_four_jobs_success':True},'public_windows_files_unchanged':True,'limits':['Local preview; no official GitHub Release or Store submission','Software VM/default DPI; physical GPU, extended scaling, screen-reader usability and final owner acceptance remain separate','Populated index CPU observations are with main GUI open; not personal or pure unattended workload acceptance','Full remaining roadmap audit remains required'],'cleanup':'All native fixture VMs/media/consoles removed; previous MSIX alias and superseded preview retired only after consumers; stable candidate retains current packages/payload and compact evidence.'}
temp=authority.with_suffix('.next');temp.write_text(json.dumps(current,indent=2)+'\n');temp.replace(authority)
old_remaining=[p for p in retired.rglob('*') if p.is_file()];unused(old_remaining);removed=[{'path':str(p.relative_to(retired)),'bytes':p.stat().st_size,'sha256':sha(p)} for p in old_remaining];shutil.rmtree(retired)
report={'status':'pass: exact current Windows preview promoted','source_revision':source,'published':str(destination),'artifacts':artifacts,'all58_payload_hashes_rechecked_after_promotion':True,'held_msi_preserved_sha256':diagnostic['sha256'],'removed_superseded_files':removed,'removed_superseded_logical_bytes':sum(x['bytes'] for x in removed),'temporary_stage_absent':not stage.exists(),'retired_directory_absent':not retired.exists(),'previous_msix_alias_removed_after_both_consumers':True,'scope':'Stable local preview; current MSI remains unaccepted and public releases unchanged'}
(e/'DELIVERY.json').write_text(json.dumps(report,indent=2)+'\n')
b=read(e/'windows/BUILD_SOURCE.json');b.update(pending_consumers=[],cleanup_complete=True,delivery_status='Accepted whole Windows preview promoted; staging and previous alias removed');(e/'windows/BUILD_SOURCE.json').write_text(json.dumps(b,indent=2)+'\n')
print(json.dumps({k:report[k] for k in ['status','source_revision','removed_superseded_logical_bytes']}))
