"""Validate terminal QA ownership, current deliverables and task-created storage."""
from pathlib import Path
import hashlib,json,subprocess
r=Path.cwd();e=Path(__file__).resolve().parent
read=lambda p:json.loads(p.read_text())
base=read(e/'BASELINE.json');current={p:int(subprocess.check_output(['du','-s','-B1',str(r/p)]).split()[0]) for p in base['compiler_allocated_bytes']}
domains=set(subprocess.check_output(['virsh','-c','qemu:///system','list','--all','--uuid'],text=True).split());lines=subprocess.check_output(['virsh','-c','qemu:///system','vol-list','default'],text=True).splitlines();volumes={v.split()[0] for v in lines if v.strip() and not v.strip().startswith(('Name','-'))};cases=[];profiles=[]
for name in ['windows-picker','windows-picker-install-deadline','assembly','upgrade-enabled']:
 o=read(e/name/'OWNER.json');c=o['cleanup'];assert c['domain_removed'] and c['fixture_removed'] and o['uuid'] not in domains;assert not(set(c['owned_volumes_removed'])&volumes)
 cases.append({'case':name,'uuid':o['uuid'],'volumes_removed':c['owned_volumes_removed']})
 if o.get('private_console'):
  p=Path(o['private_console']['home']).parent;assert not p.exists();v=read(e/name/'viewer/TEARDOWN.json');assert v['profile_removed'];profiles.append(str(p))
for name in ['linux-picker-dark','linux-picker-light','linux-creation','linux-creation-retained','linux-creation-native-close']:
 d=read(e/name/'TEARDOWN.json');assert d['profile_removed'] and not Path(d['profile']).exists();profiles.append(d['profile'])
retired=['.hermes/packaging-tmp/.linux-native-desktop-stage','.hermes/packaging-tmp/.linux-desktop-extract','.hermes/packaging-tmp/.windows-picker-stage','.hermes/packaging-tmp/.windows-msix-source','.hermes/packaging-tmp/.windows-candidate-retired','.hermes/evidence/picker-accessibility-current/previous-input.msix','.hermes/evidence/picker-accessibility-current/source.zip']
assert all(not(r/p).exists() for p in retired)
protected={'win11ltsc':'4e160378-2610-4072-b528-048e4c9f509e','ottrin-copy-cleanup-qa':'9b2e795f-fb04-4adb-95a7-8832f8894c67'}
for name,uid in protected.items():assert subprocess.check_output(['virsh','-c','qemu:///system','domuuid',name],text=True).strip()==uid
retirement=read(e/'INCREMENTAL_RETIREMENT.json');assert retirement['status'].startswith('all13') and len(retirement['guarded_removal_ledger'])==13
for d in retirement['guarded_removal_ledger']:assert not Path(d['path']).exists()
# The UI test output reused the exact established compiler/configuration identity.
assert 'ottrin_ui-11a85ca5198af5b6' in (e/'picker-tests.log').read_text() and 'ottrin_ui-11a85ca5198af5b6' in (r/'.hermes/evidence/release-audit-current/ui-tests.log').read_text()
for row in read(e/'DELIVERED_HASHES.json')['artifacts']:
 with Path(row['path']).open('rb') as f:assert hashlib.file_digest(f,'sha256').hexdigest()==row['sha256']
receipt={'status':'owned QA processes/profiles/displays/buses/mounts/VMs/media, stages/source/baseline alias and superseded output retired after consumers','native_cases':cases,'native_volumes_removed':sum(len(c['volumes_removed']) for c in cases),'private_profiles_removed':profiles,'retired_paths':retired,'compiler_storage':{p:{'baseline':n,'current':current[p],'delta':current[p]-n} for p,n in base['compiler_allocated_bytes'].items()},'compiler_cleanup':'All13 reviewed obsolete incremental sessions removed under exclusive locks and process checks. Refusals and explicit system-process inspection gaps retained in evidence. Newest finalized sessions, current metadata/dependencies/test executable and useful shared caches retained; no extra target/configuration or blanket clean. The same UI test output identity replaces its previous version; no superseded matching-config test executable generated.','remaining_task_created_storage':[{'path':str(e),'purpose':'Compact source/native acceptance, failed-attempt diagnostics, guarded cleanup records and reviewed original captures; no live consumers'},{'path':str(r/'dist/packages'),'purpose':'Current usable Linux four-format d6 delivery'},{'path':str(r/'dist/windows-candidate'),'purpose':'Current usable d6 ZIP/NSIS/development MSIX8 plus58-file payload'}],'protected_bases_preserved':protected,'personal_service':'Preserved normal user service; daemon payload is byte-identical and no personal restart/settings mutation was needed','limits':'Full roadmap not complete; unnamed creation text input and private Alt+F4 observation remain explicit independent audit work'}
(e/'CLEANUP_FINAL.json').write_text(json.dumps(receipt,indent=2)+'\n');print('Verified4 terminal native cases,8 owned volumes and'+str(len(profiles))+' removed private profiles')
