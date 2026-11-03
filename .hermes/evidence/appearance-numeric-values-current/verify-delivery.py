"""Verify stable publication, exact payloads, held diagnostic and all owned cleanup."""
from pathlib import Path
import hashlib,json,subprocess
r=Path.cwd();e=Path(__file__).resolve().parent;read=lambda p:json.loads(p.read_text())
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
source=read(e/'BUILD_SOURCE.json')['source_revision'];lin=read(r/'dist/packages/LINUX_PACKAGES_ACCEPTANCE.json');win=read(r/'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json');assert lin['source_revision']==win['source_commit']==source
artifacts={}
for root,receipt in [(r/'dist/packages',lin),(r/'dist/windows-candidate',win)]:
 for name,digest in receipt['artifacts'].items():
  p=root/name;assert sha(p)==digest;assert (root/(name+'.sha256')).read_text().split()[0]==digest
  artifacts[str(p.relative_to(r))]={'sha256':digest,'bytes':p.stat().st_size,'sidecar_matches':True}
assert len(artifacts)==7
payload=read(e/'PAYLOAD.json');folder=r/'dist/windows-candidate/windows-payload/Ottrin-0.1.0-windows-x64';actual={str(p.relative_to(folder)):sha(p) for p in folder.rglob('*') if p.is_file()};assert actual==payload and len(actual)==58
held=read(r/'.hermes/evidence/windows-msi-package-control-current/input/DIAGNOSTIC_INPUT.json');assert sha(Path(held['artifact']))==held['sha256']
absent=[r/'.hermes/packaging-tmp'/n for n in ['.linux-native-desktop-stage','.windows-picker-stage','.windows-candidate-retired','.linux-desktop-extract','.windows-msix-source']]+[e/'source.zip'];assert all(not p.exists() for p in absent)
vm=[];profiles=[]
for label in ['native','assembly']:
 o=read(e/label/'OWNER.json');assert o['cleanup']['domain_removed'] and o['cleanup']['fixture_removed'] and len(o['cleanup']['owned_volumes_removed'])==2
 assert subprocess.run(['virsh','-c','qemu:///system','domuuid',o['domain']],capture_output=True).returncode!=0
 for name in o['cleanup']['owned_volumes_removed']:assert subprocess.run(['virsh','-c','qemu:///system','vol-path','--pool','default',name],capture_output=True).returncode!=0
 for name in ['RESULTS.zip','controller.ps1','GUEST_PREPARATION.json','CONTROLLER_PROCESS.json','DESKTOP_SIGNAL.json','readiness.log','desktop-ready.png','PRIVACY_STEPS.json']:assert not (e/label/name).exists()
 assert not list((e/label).glob('privacy-accepted-*.png'))
 vm.append({'label':label,'uuid':o['uuid'],'cleanup':o['cleanup']})
 if 'private_console' in o:
  p=Path(o['private_console']['home']).parent;assert not p.exists();profiles.append(str(p))
for label in ['dark100','light150']:
 t=read(e/label/'TEARDOWN.json');assert t['profile_removed'] and not Path(t['profile']).exists();profiles.append(t['profile'])
assert len(profiles)==3
allowed={'ottrin-copy-cleanup-qa','ottrin-index-qa','ottrin-qa-20261011','win11ltsc'};domains=set(subprocess.check_output(['virsh','-c','qemu:///system','list','--all','--name'],text=True).split());assert domains==allowed
storage=read(e/'COMPILER_CLEANUP.json');assert 'identified: 0' in (e/'incremental-final-inspection.log').read_text()
verification={'status':'pass exact stable publication and scoped ownership cleanup','source_revision':source,'artifacts':artifacts,'windows_payload_files':58,'windows_payload_hashes_match':True,'held_msi_preserved_sha256':held['sha256'],'limits':['Functional local preview; full roadmap remains open','Strict software GUI idle CPU gate failed; no waiver','Windows Dark144DPI software native scope verified; other scale/theme combinations and physical graphics remain unverified','MSIX13 assembly only; prior lifecycle retains original scope']}
(e/'DELIVERY_VERIFICATION.json').write_text(json.dumps(verification,indent=2)+'\n')
cleanup={'status':'complete for current named-control and native DPI task resources','owned_vms':vm,'removed_private_profiles':profiles,'removed_stages_extractions_archives':[str(p) for p in absent],'consumed_input_batches':read(e/'CONSUMED_INPUTS_RETIRED.json'),'superseded_windows':{'files_removed':len(read(e/'WINDOWS_DELIVERY.json')['removed_superseded_files']),'logical_bytes':read(e/'WINDOWS_DELIVERY.json')['removed_superseded_logical_bytes']},'superseded_linux':'Replaced at stable paths through production artifact lifecycle after hash/link/open-consumer checks; no versioned duplicate retained','compiler_storage_comparison':storage,'obsolete_incremental_sessions_removed_earlier':15,'final_obsolete_incremental_candidates':0,'protected_domains_retained':sorted(domains),'remaining_task_created_storage':[{'path':str(e),'purpose':'Compact acceptance receipts, frozen producers, original functional captures and raw diagnostic logs; current evidence retained'},{'path':str(r/'.hermes/tools/windows-display-driver'),'purpose':'Small reusable signed display-driver input for current and future isolated native DPI acceptance; useful cache retained'},{'path':str(r/'dist/packages'),'purpose':'Four current usable Linux deliverables and acceptance manifest'},{'path':str(r/'dist/windows-candidate'),'purpose':'Current ZIP/NSIS/MSIX13 and58-file payload for delivery/identity consumers'}],'pending_disposable_consumers':[],'scope':'Personal settings/data/services and shared useful compiler caches retained; original strict CPU failure remains recorded separately.'}
(e/'CLEANUP_FINAL.json').write_text(json.dumps(cleanup,indent=2)+'\n');print('Verified7artifacts+sidecars+58payloads;2VMs/4volumes/3profiles and task stages retired')
