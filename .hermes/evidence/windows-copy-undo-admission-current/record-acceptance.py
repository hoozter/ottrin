"""Bind source verification to the completed native runs; never accepts packages."""
from pathlib import Path
import json, hashlib, subprocess, gzip
p=Path(__file__).resolve().parent
r=Path.cwd()
def contents(name):
 f=p/name
 return f.read_text() if f.exists() else gzip.decompress(f.with_name(f.name+'.gz').read_bytes()).decode()
def read(name): return json.loads(contents(name))
def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()
for folder in ['', 'identity-probe/', 'pipe/']:
 owner=read(folder+'RUNTIME_OWNER.json')
 assert owner['status']=='pass' and owner['guest_off'] and owner['root_removed']
 for name,digest in owner['source']['files'].items(): assert sha(r/name)==digest,name
 teardown=read(folder+'RUNTIME_TEARDOWN.json')
 assert teardown['ownedRootRemoved'] and teardown['ownedProcesses']==0 and teardown['originalProfilesPreserved']
 for role in ['ordinary','elevated']:
  log=contents(folder+f'short-temp-{role}-runtime.stdout')
  expected='222 passed; 0 failed; 6 ignored; 0 measured; 0 filtered out;' if folder=='pipe/' else '1 passed; 0 failed; 0 ignored;' if folder else '375 passed; 0 failed; 4 ignored; 0 measured; 0 filtered out;'
  assert expected in log,(folder,role)
  if not folder:
   for suffix,count in [('-copy-native.stdout',4),('-native.stdout',1),('-copy-activity.stdout',1)]:
    assert f'test result: ok. {count} passed; 0 failed;' in contents(f'short-temp-{role}-runtime{suffix}')
assert subprocess.check_output(['virsh','-c','qemu:///system','domstate','ottrin-copy-cleanup-qa'],text=True).strip()=='shut off'
assert '402 passed; 0 failed; 1 ignored;' in contents('linux-ui.stdout')
files=read('pipe/RUNTIME_OWNER.json')['source']['files']
receipt={'status':'pass for exact source and named native scopes; package and full hosted verification pending',
 'base_revision':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'source_files':files,
 'copy_undo':{'ordinary_and_elevated_each':{'regular_ui_passed':375,'existing_ui_ignores':4,'regular_ui_filtered':0,'copy_session_controls':4,'move_authority_controls':1,'ignored_activity_matrix_cases':29},'scope':'Fresh native admission resolves renamed parents with original completion authority; changed bytes, old-name occupants, missing authority, cancellation and partial batches retain strict checks. Centralized completion refresh repairs refused-admission Activity state. Queue claim test uses canonical input spelling; no universal 8.3 alias contract.'},
 'search_identity':{'ordinary_and_elevated_each':{'passed':222,'existing_ignores':6,'filtered':0},'focused_identity_each':1,'strict_handle_equality':True,'positive_owned_handle_control':True,'scope':'Dedicated exact-filter child avoids unrelated parallel-test handles. Warm the same success/error paths once before measurement;100 subsequent iterations must retain exact count. No production authentication change.'},
 'linux_ui':{'passed':402,'existing_ignores':1},
 'lint':'Linux and Windows GNU UI/platform all-target strict lint; Windows GNU search strict lint',
 'hosted_previous_failure':{'run':40278830449,'source':'3ee5f7c9','scope':'Linux and benchmark pass; Windows221 search passes,one process-global handle-count failure,6 existing ignores. Subsequent Windows lint/release skipped.'},
 'rejected_controls':['changed-admission-state-control','short-path-claim-control','handle-probe-startup-control'],
 'teardown':'All three native runs removed owned roots/processes, preserved original profiles and shut guest down with unchanged XML. Personal Linux service and current packages preserved.',
 'delivery':'No new package acceptance; Linux defc1ada and internal Windows bcd625ac remain current.'}
(p/'ACCEPTANCE.json').write_text(json.dumps(receipt,indent=2)+'\n')
print('Source/native acceptance bound to current files; packages remain separate')
