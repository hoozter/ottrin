"""Verify exact public caller cleanup/pending/discovery and ordinary controls."""
from pathlib import Path
import hashlib,json,re,subprocess,zipfile
r=Path(__file__).resolve().parent;e=r/'native'
h=lambda p:hashlib.file_digest(open(p,'rb'),'sha256').hexdigest()
s=json.loads((r/'SOURCE_INPUT.json').read_text());b=json.loads((r/'NATIVE_TEST_INPUT.json').read_text());n=json.loads((e/'APPLICATION_INPUT.json').read_text())
assert n['changed_product_sources']==s['files'] and n['source_revision']==s['reference_commit']
for p,w in s['files'].items():assert h(p)==w,p
for a in b['artifacts']:assert h(a['executable'])==a['sha256']
a=next(x for x in b['artifacts'] if x['crate']=='ottrin_platform');assert (n['source_test_sha256'],n['source_bytes'])==(a['sha256'],a['bytes'])
for p,key in [('actor.ps1','actor_sha256'),('system-companion.ps1','system_companion_sha256')]:assert h(r/p)==n[key]
archive=e/'RESULTS.zip'
with zipfile.ZipFile(archive) as z:
 members=sorted(z.namelist());assert members==sorted(['CALLER_READY.json','DESKTOP_READY.json','OWNER.json','OWNER_OBSERVATION.json','RESULT.json','SYSTEM_PROCESS.json','SYSTEM_RESULT.json'])
 records={Path(name).stem:json.loads(z.read(name).decode('utf-8-sig')) for name in members}
for name,v in records.items():(e/('GUEST_OWNER.json' if name=='OWNER' else name+'.json')).write_text(json.dumps(v,indent=2)+'\n')
g=json.loads((e/'GUEST_RESULT.json').read_text());system=records['SYSTEM_RESULT'];assert g==records['RESULT'] and g['status'].startswith('pass') and system['status'].startswith('pass') and g['system']==system
assert len(g['runs'])==1 and g['runs'][0]['case']=='ordinary cross-principal client'
plan=json.loads((r/'REGRESSION_PLAN.json').read_text());assert [x['case'] for x in system['runs']]==['caller private helper']+plan['tests']
runs=g['runs']+system['runs'];assert len(runs)==plan['expected_top_level_invocations']==16
for run in runs:
 assert run['exit']==0 and not run.get('timed_out',False),run
 assert re.findall(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored',run['stdout'])[-1]==('1','0'),run
for run in system['runs'][1:]:
 prefix='test copy_transaction::windows::journal::tests::cleanup::'+run['case']+' ... '
 assert re.search(re.escape(prefix)+r'(?:ok|[\s\S]*\nok)\n\ntest result: ok\. 1 passed; 0 failed; 0 ignored',run['stdout']),run['case']
helper=system['runs'][0]
for directory in ['false','true']:
 assert f'caller public cleanup source_directory={directory}: single-member admission, cancellation, original cohort IDs, idempotent retirement and foreign-root refusal passed' in helper['stdout']
 for interrupted in ['false','true']:
  assert f'caller public cohort source_directory={directory} interrupted={interrupted}: canonical admission, readonly original-cohort recovery and idempotent retirement passed' in helper['stdout']
  assert f'caller public shared cohort source_directory={directory} interrupted={interrupted}: canonical admission, cancellation, physical-unlink recovery and idempotent retirement passed' in helper['stdout']
  assert f'caller shared cohort source_directory={directory} interrupted={interrupted}: caller-owned aliases, coordinated unlink, fresh recovery, outside-link preservation and ordered retirement passed' in helper['stdout']
 for undo in ['false','true']:
  assert f'caller cleanup discovery source_directory={directory} undo={undo}: readonly payload/metadata status, cancellation and journal preservation passed' in helper['stdout']
  assert f'caller metadata source_directory={directory} undo={undo}: paired intent, read-only fresh admission, private authority, ordered removal and idempotent replay passed' in helper['stdout']
o=json.loads((e/'OWNER.json').read_text());assert all(o['cleanup'][k] is True for k in ['domain_removed','fixture_removed','base_still_off']);assert o['cleanup']['retained_pending_volume'] is None
for pid in [o['private_console']['pid'],o.get('desktop_observer_pid')]:
 if pid:assert not Path('/proc',str(pid)).exists(),pid
assert not Path(o['private_console']['home']).parent.exists()
domains=subprocess.check_output(['virsh','-c','qemu:///system','list','--all'],text=True);volumes=subprocess.check_output(['virsh','-c','qemu:///system','vol-list','default'],text=True);assert o['domain'] not in domains and o['domain'] not in volumes
v={'status':'pass public caller cleanup/pending/discovery and affected ordinary controls','source':'SOURCE_INPUT.json','build':'NATIVE_TEST_INPUT.json','native_invocations':16,'test_passes':16,'caller_public_cleanup_flows':10,'caller_payload_metadata_discovery_pairs':4,'ordinary_tests':plan['tests'],'archive_sha256':h(archive),'archive_members':members,'independent_teardown':{'domains':domains,'volumes':volumes,'console_observer_profile_absent':True},'limits':['Original client is still live; recaptured contexts do not prove actual client/helper restart or new authenticated connections','Full caller dispatcher, complete failure/readonly/symbolic-link matrices, TokenOwner mutation and alternate-admin UAC/package/profile gates remain required','Administrator Copy stays disabled; stable packages unchanged; hosted acceptance separate']}
(r/'VERIFICATION.json').write_text(json.dumps(v,indent=2)+'\n')
(r/'BUILD_BINDING.json').write_text(json.dumps({'status':'exact source/build/staged executable text/native result binding passed','source':'SOURCE_INPUT.json','build':'NATIVE_TEST_INPUT.json','native':'native/APPLICATION_INPUT.json','unchanged_executable_text':n['unchanged_executable_text']},indent=2)+'\n')
v['coverage']=['Admitted WindowsCopyRegistry exposes cleanup/pending plans using retained authority and canonical profile admission; ordinary public entry points select Process once','Shared discovery observes caller payload/metadata cleanup without mutation and preserves cancellation; existing identity/security/content checks remain','Actual file/directory single, mixed and shared cohorts finish through public plans; original-cohort pending recovery works after local removal; invalid authority/foreign registry/cancelled admission refuse; outside aliases/public trees/source/preferences/index remain preserved','Fourteen exact ordinary public configured/pending/discovery parents cover interruption, unknown namespace data, native path losslessness, torn checkpoints, foreign ACL/log changes and new-work fencing']
v['lint']={'command':'cargo clippy -p ottrin-platform -p ottrin-ui --target x86_64-pc-windows-gnu --all-targets -- -D warnings','exit':0,'sha256':h(r/'windows-lint.log')};(r/'ACCEPTANCE.json').write_text(json.dumps(v,indent=2)+'\n');print('PASS 16 native test passes/16 invocations; 10 caller public cleanup flows, four discovery pairs and independent teardown verified.')
