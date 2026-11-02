"""Deliver the verified preview cohort; retain only the exact held MSI diagnostic input."""
from pathlib import Path
import hashlib,json,shutil,subprocess,zipfile
r=Path.cwd();e=Path(__file__).resolve().parent;stage=r/'.hermes/packaging-tmp/.windows-picker-stage';destination=r/'dist/windows-candidate';retired=r/'.hermes/packaging-tmp/.windows-candidate-retired'
held=r/'.hermes/evidence/windows-msi-package-control-current/input';receipt_path=r/'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json'
assert not(e/'DELIVERY.json').exists() and not retired.exists() and not held.exists()
def read(p):return json.loads(p.read_text())
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
old=read(receipt_path);host=read(e/'HOST_PAYLOAD.json');assembly=read(e/'ASSEMBLY_ACCEPTANCE.json');native=read(e/'nsis-native-hidden-console/ACCEPTANCE.json');transport=read(e/'transport/TRANSPORT_ACCEPTANCE.json')
assert old['source_commit']=='8ede156248e54af88e972d53fe441b05af9caa3c'
assert native['status'].startswith('pass within exact ordinary-user') and transport['status']=='pass'
assert read(e/'nsis-native-hidden-console/VISUAL_REVIEW.json')['status']=='reviewed'
for case in ['upgrade-enabled','upgrade-disabled']:assert read(e/case/'ACCEPTANCE.json')['status'].startswith('pass within actual native')
assert native['source_revision']==assembly['source_revision']==host['source_revision']
source=host['source_revision'];product_paths=['Cargo.toml','Cargo.lock','crates','assets','packaging','scripts','build-all.sh']
subprocess.run(['git','diff','--exit-code','bf3abfd32fc7654108e3f723f6a95c2f5866c5af',source,'--',*product_paths],check=True,stdout=subprocess.DEVNULL)
artifacts=host['artifacts']|{Path(assembly['artifact']).name:assembly['artifact_sha256']}
for name,digest in artifacts.items():assert sha(stage/name)==digest
payload=read(e/'PAYLOAD.json');assert len(payload)==58
for name,digest in payload.items():assert sha(stage/'windows-payload/Ottrin-0.1.0-windows-x64'/name)==digest
old_payload_root=destination/'windows-payload/Ottrin-0.1.0-windows-x64';old_payload={str(p.relative_to(old_payload_root)):sha(p) for p in old_payload_root.rglob('*') if p.is_file()};assert len(old_payload)==58
for name,digest in old['artifacts'].items():assert sha(destination/name)==digest
with zipfile.ZipFile(destination/'Ottrin-0.1.0-windows-x64-portable.zip') as z:
 zipped={str(Path(x.filename).relative_to('Ottrin-0.1.0-windows-x64')):hashlib.sha256(z.read(x)).hexdigest() for x in z.infolist() if not x.is_dir()}
assert zipped==old_payload
old_files=[p for p in destination.rglob('*') if p.is_file()];assert all(not p.is_symlink() and p.stat().st_nlink==1 for p in old_files)
known={str(Path('windows-payload/Ottrin-0.1.0-windows-x64')/name) for name in old_payload}|set(old['artifacts'])|{name+'.sha256' for name in old['artifacts']}
assert {str(p.relative_to(destination)) for p in old_files}==known,'Unexpected files in previous candidate; preserve them'
assert subprocess.run(['fuser',*map(str,old_files)],capture_output=True).returncode==1
provenance=read(e/'assembly/ASSEMBLY.json')['provenance'];assert provenance['version']=='0.1.0.4' and not provenance['signed'] and provenance['sha256']==assembly['artifact_sha256']
(stage/(Path(assembly['artifact']).name+'.provenance.json')).write_text(json.dumps(provenance,indent=2)+'\n')
for name,digest in artifacts.items():(stage/(name+'.sha256')).write_text(digest+'  '+name+'\n')
(stage/'PREVIEW_STATUS.md').write_text('''# Current Windows preview

This directory contains the verified portable ZIP, per-user NSIS installer and
development MSIX from source `'''+source+'''`.

Portable/NSIS native chooser, fresh installation/removal, running-index upgrade,
startup/running intent preservation and real uninstall refusal/retry pass. The
MSIX passes real upgrades, reboot/login startup choices and removal. The native
GUI checks use software rendering at default Windows DPI; physical graphics,
further scaling and final owner acceptance remain separate.

The development MSIX uses the Ottrin Development identity and was tested with
administrative `Add-AppxPackage -AllowUnsigned` deployment. Store distribution
remains pending. The portable ZIP and NSIS are the available ordinary-user previews.

A current MSI is held on the unresolved ordinary-user Windows Installer rollback
boundary. Its exact older diagnostic input is retained under the project evidence;
it is not included as a current usable preview. See `../WINDOWS_CANDIDATE_ACCEPTANCE.json`.
''')
held.mkdir();(held/'ORIGINAL_CANDIDATE_ACCEPTANCE.json').write_bytes(receipt_path.read_bytes());(held/'payload.json').write_text(json.dumps(old_payload,indent=2)+'\n')
msi_name='Ottrin-0.1.0-windows-x64.msi';held_msi=held/msi_name
held_receipt={'status':'Retained exact failed ordinary-user MSI for the explicit native rollback boundary; not a current preview','artifact':str(held_msi),'sha256':old['artifacts'][msi_name],'source_revision':old['source_commit'],'payload_manifest':str(held/'payload.json'),'payload_files':58,'pending_consumer':'A supported Windows Installer correction or explicitly approved installation-contract change must be evaluated against this known failing input','evidence':'.hermes/evidence/windows-msi-package-control-current/verified-teardown/ASSESSMENT.json'}
(e/'PROMOTION_INTENT.json').write_text(json.dumps({'source_revision':source,'from':str(stage),'to':str(destination),'temporary_old':str(retired),'held_input':held_receipt,'status':'Inputs verified; directory promotion intent recorded before moves'},indent=2)+'\n')
swapped=False;msi_moved=False
try:
 (destination/msi_name).rename(held_msi);msi_moved=True
 (held/'DIAGNOSTIC_INPUT.json').write_text(json.dumps(held_receipt,indent=2)+'\n')
 destination.rename(retired)
 try:stage.rename(destination)
 except BaseException:retired.rename(destination);raise
 swapped=True
 for name,digest in artifacts.items():assert sha(destination/name)==digest
 for name,digest in payload.items():assert sha(destination/'windows-payload/Ottrin-0.1.0-windows-x64'/name)==digest
 assert sha(held_msi)==held_receipt['sha256']
 current={'status':'pass within recorded local Windows preview scopes; required MSI release held separately','source_commit':source,'working_tree_at_build':'clean','candidate':str(destination),'artifacts':artifacts,'payload_files':58,'evidence_directory':str(e.relative_to(r)),'host_content':'HOST_PAYLOAD.json','ordinary_nsis_and_portable':str((e/'nsis-native-hidden-console/ACCEPTANCE.json').relative_to(r)),'rendered_review':str((e/'nsis-native-hidden-console/VISUAL_REVIEW.json').relative_to(r)),'native_msix':{case:str((e/case/'ACCEPTANCE.json').relative_to(r)) for case in ['upgrade-enabled','upgrade-disabled']},'native_msi_rollback':{'status':'held: ordinary-user registration rollback/retry failed on exact8ede diagnostic; no current accepted MSI','release_accepted':False,'diagnostic_input':str((held/'DIAGNOSTIC_INPUT.json').relative_to(r))},'hosted_source':{'run':41508619836,'tested_source_revision':'bf3abfd32fc7654108e3f723f6a95c2f5866c5af','product_inputs_identical':True},'public_windows_files_unchanged':True,'limits':['Local preview delivery; no official GitHub Release or Store submission','Native software Windows desktop at default DPI; physical GPU, further scaling and final owner acceptance remain separate','Remaining software/platform/assistive acceptance is audited independently; this receipt does not complete the roadmap'],'cleanup':'Superseded candidate files retired after all named consumers complete; sole failing MSI retained for its explicit native boundary; see evidence DELIVERY.json and CLEANUP.json'}
 temporary=receipt_path.with_suffix('.next');temporary.write_text(json.dumps(current,indent=2)+'\n');temporary.replace(receipt_path)
 old_remaining=[p for p in retired.rglob('*') if p.is_file()];assert all(p.stat().st_nlink==1 for p in old_remaining);assert subprocess.run(['fuser',*map(str,old_remaining)],capture_output=True).returncode==1
 removed=[{'path':str(p.relative_to(retired)),'bytes':p.stat().st_size,'sha256':sha(p)} for p in old_remaining]
 shutil.rmtree(retired)
 report={'status':'pass: exact verified current Windows preview promoted to stable candidate; MSI held separately','source_revision':source,'published':str(destination),'artifacts':artifacts,'all58_payload_hashes_rechecked_after_promotion':True,'held_msi':held_receipt,'removed_superseded_files':removed,'removed_superseded_logical_bytes':sum(p['bytes'] for p in removed),'temporary_stage_absent':not stage.exists(),'retired_directory_absent':not retired.exists(),'previous_nsis_test_input_retired_after_consumer_complete':True,'replay_limit':'The native assessors ran against the exact old/new packages before superseded inputs retired. Published current files are rechecked here against those accepted hashes; replay of the older upgrade requires its exact recorded baseline.','scope':'Stable local preview; no failed MSI relabelled or ordinary-user contract changed; official/public releases unchanged'}
 (e/'DELIVERY.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({'status':report['status'],'source_revision':source,'removed_superseded_bytes':report['removed_superseded_logical_bytes']}))
except BaseException:
 if not swapped and msi_moved and not(destination/msi_name).exists() and held_msi.exists():held_msi.rename(destination/msi_name)
 raise
