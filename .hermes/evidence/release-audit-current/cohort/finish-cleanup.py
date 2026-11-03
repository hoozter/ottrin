"""Verify exact completed resource ownership and retained current deliverables."""
from pathlib import Path
import json,subprocess
r=Path.cwd();e=r/'.hermes/evidence/release-audit-current';cohort=Path(__file__).resolve().parent
baseline=json.loads((cohort/'BASELINE.json').read_text())
current={n:int(subprocess.check_output(['du','-s','-B1',str(r/n)],text=True).split()[0]) for n in baseline['allocated_bytes']}
domains=set(subprocess.check_output(['virsh','-c','qemu:///system','list','--all','--uuid'],text=True).split())
lines=subprocess.check_output(['virsh','-c','qemu:///system','vol-list','default'],text=True).splitlines()
volumes={s.split()[0] for s in lines if s.strip() and not s.strip().startswith(('Name','-'))}
cases=[]
for name in ['windows-startup','windows-startup-controls','windows-startup-viewport','assembly','upgrade-enabled','upgrade-enabled-input','upgrade-enabled-console','upgrade-disabled']:
 o=json.loads((cohort/name/'OWNER.json').read_text());c=o['cleanup'];assert c['domain_removed'] and c['fixture_removed'] and o['uuid'] not in domains
 assert not(set(c['owned_volumes_removed']) & volumes)
 cases.append({'case':name,'uuid':o['uuid'],'owned_volumes_removed':c['owned_volumes_removed']})
profiles=[]
paths=[cohort/n/'viewer' for n in ['windows-startup','windows-startup-controls','windows-startup-viewport','upgrade-enabled-input','upgrade-enabled-console','upgrade-disabled']]
paths += [cohort/n for n in ['startup-dark-1','startup-light-1.5']]
paths += [e/n for n in ['linux-evidence-root-cancelled','linux-packaged-final','linux-packaged-motion','linux-packaged-integrity','linux-packaged-integrity-diagnostic','linux-packaged-retained-integrity']]
for case in paths:
 a=json.loads((case/'TEARDOWN.json').read_text());assert a['profile_removed'] and not Path(a['profile']).exists();profiles.append(a['profile'])
retired=['.hermes/packaging-tmp/.linux-native-desktop-stage','.hermes/packaging-tmp/.linux-desktop-extract','.hermes/packaging-tmp/.windows-picker-stage','.hermes/packaging-tmp/.windows-msix-source','.hermes/packaging-tmp/.windows-candidate-retired','.hermes/evidence/release-audit-current/cohort/previous-input.msix','.hermes/evidence/release-audit-current/cohort/source.zip']
assert all(not(r/p).exists() for p in retired)
protected={'win11ltsc':'4e160378-2610-4072-b528-048e4c9f509e','ottrin-copy-cleanup-qa':'9b2e795f-fb04-4adb-95a7-8832f8894c67'}
for name,uid in protected.items():assert subprocess.check_output(['virsh','-c','qemu:///system','domuuid',name],text=True).strip()==uid
receipt={'status':'all cohort fixtures/staging/source/archive/baseline aliases retired after named consumers','native_cases':cases,'native_volumes_removed':sum(len(c['owned_volumes_removed']) for c in cases),'private_profiles_removed':profiles,'retired_paths':retired,'compiler_storage':{name:{'baseline':n,'current':current[name],'delta':current[name]-n} for name,n in baseline['allocated_bytes'].items()},'cargo_scope':'No Rust compilation in this continuation; no new Cargo configuration, test executable, fingerprint or incremental session generated. Existing shared/current caches preserved. Prior exact incremental cleanup remains separately receipted.','remaining_task_created_storage':[{'path':str(e),'purpose':'Compact acceptance/failure receipts, source producers and original native product captures; retained evidence, no pending process'},{'path':str(r/'dist/packages'),'purpose':'Current usable Linux four-format7d delivery'},{'path':str(r/'dist/windows-candidate'),'purpose':'Current usable7d ZIP/NSIS/development MSIX7 and58-file payload'}],'personal_service':'Supported refresh intentionally retained as the normal user service, not a QA process. Saved preferences unchanged; exact delivered daemon verified.','protected_bases_preserved':protected,'no_public_release_or_notification_settings_changed':True}
(cohort/'CLEANUP_FINAL.json').write_text(json.dumps(receipt,indent=2)+'\n')
print('Verified',len(cases),'terminal case records,',receipt['native_volumes_removed'],'owned volumes and',len(profiles),'removed private profiles')
