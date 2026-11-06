"""Verify caller shared aliases and individually retained ordinary cohort tests."""
from pathlib import Path
import hashlib,json,re,subprocess,zipfile
r=Path(__file__).resolve().parent;e=r/'shared-native'
h=lambda p:hashlib.file_digest(open(p,'rb'),'sha256').hexdigest()
s=json.loads((r/'SHARED_SOURCE_INPUT.json').read_text());old=json.loads((r/'SOURCE_INPUT.json').read_text());b=json.loads((r/'SHARED_NATIVE_TEST_INPUT.json').read_text());n=json.loads((e/'APPLICATION_INPUT.json').read_text())
assert n['changed_product_sources']==s['files'] and n['source_revision']==s['reference_commit']
changed=[p for p in old['files'] if old['files'][p]!=s['files'][p]]
assert changed==s['only_post_focused_changes']==['crates/ottrin-platform/src/copy_transaction/windows/journal/caller.rs']
assert '#[cfg(test)]\nmod caller;' in Path('crates/ottrin-platform/src/copy_transaction/windows/journal.rs').read_text()
for p,w in s['files'].items():assert h(p)==w,p
for a in b['artifacts']:assert h(a['executable'])==a['sha256']
a=next(x for x in b['artifacts'] if x['crate']=='ottrin_platform');assert (n['source_test_sha256'],n['source_bytes'])==(a['sha256'],a['bytes'])
for p,key in [('shared-actor.ps1','actor_sha256'),('shared-system.ps1','system_companion_sha256')]:assert h(r/p)==n[key]
archive=e/'RESULTS.zip'
with zipfile.ZipFile(archive) as z:
 members=sorted(z.namelist());assert members==sorted(['CALLER_READY.json','DESKTOP_READY.json','OWNER.json','OWNER_OBSERVATION.json','RESULT.json','SYSTEM_PROCESS.json','SYSTEM_RESULT.json'])
 records={Path(name).stem:json.loads(z.read(name).decode('utf-8-sig')) for name in members}
for name,v in records.items():(e/('GUEST_OWNER.json' if name=='OWNER' else name+'.json')).write_text(json.dumps(v,indent=2)+'\n')
g=json.loads((e/'GUEST_RESULT.json').read_text());system=records['SYSTEM_RESULT'];assert g==records['RESULT'] and g['status'].startswith('pass') and system['status'].startswith('pass') and g['system']==system
assert len(g['runs'])==1 and g['runs'][0]['case']=='ordinary cross-principal client'
plan=json.loads((r/'REGRESSION_PLAN.json').read_text());expected=plan['additional_ordinary_group_cohort_tests'];assert [x['case'] for x in system['runs']]==['caller private helper']+expected
runs=g['runs']+system['runs'];assert len(runs)==44
for run in runs:
 assert run['exit']==0 and not run.get('timed_out',False),run
 assert re.findall(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored',run['stdout'])[-1]==('1','0'),run
for run in system['runs'][1:]:
 prefix='test copy_transaction::windows::journal::tests::cleanup::'+run['case']+' ... '
 assert re.search(re.escape(prefix)+r'(?:ok|[\s\S]*\nok)\n\ntest result: ok\. 1 passed; 0 failed; 0 ignored',run['stdout']),run['case']
helper=system['runs'][0]
for directory in ['false','true']:
 for interrupted in ['false','true']:
  assert f'caller shared cohort source_directory={directory} interrupted={interrupted}: caller-owned aliases, coordinated unlink, fresh recovery, outside-link preservation and ordered retirement passed' in helper['stdout']
  assert f'caller cohort source_directory={directory} interrupted={interrupted}: mixed payloads, canonical fresh admission, cancellation, private log lifecycle, ordered metadata recovery and public preservation passed' in helper['stdout']
 for undo in ['false','true']:
  assert f'caller metadata source_directory={directory} undo={undo}: paired intent, read-only fresh admission, private authority, ordered removal and idempotent replay passed' in helper['stdout']
o=json.loads((e/'OWNER.json').read_text());assert all(o['cleanup'][k] is True for k in ['domain_removed','fixture_removed','base_still_off']);assert o['cleanup']['retained_pending_volume'] is None
for pid in [o['private_console']['pid'],o.get('desktop_observer_pid')]:
 if pid:assert not Path('/proc',str(pid)).exists(),pid
assert not Path(o['private_console']['home']).parent.exists()
domains=subprocess.check_output(['virsh','-c','qemu:///system','list','--all'],text=True);volumes=subprocess.check_output(['virsh','-c','qemu:///system','vol-list','default'],text=True);assert o['domain'] not in domains and o['domain'] not in volumes
v={'status':'pass caller shared aliases and all additional ordinary group/cohort parents','source':'SHARED_SOURCE_INPUT.json','build':'SHARED_NATIVE_TEST_INPUT.json','only_post_focused_changes':changed,'native_invocations':44,'test_passes':44,'caller_shared_cohort_cases':4,'caller_mixed_cohort_cases':4,'ordinary_tests':expected,'archive_sha256':h(archive),'archive_members':members,'independent_teardown':{'domains':domains,'volumes':volumes,'console_observer_profile_absent':True},'limits':'Live original client recaptures and injected native failures only; public cleanup, real client/helper restart/new connections, UAC, full caller failure/readonly/link matrices, TokenOwner and package/profile contexts remain separate; Administrator Copy disabled'}
(r/'SHARED_VERIFICATION.json').write_text(json.dumps(v,indent=2)+'\n');print('PASS 44 test passes/44 invocations; four shared and four mixed caller cohorts, all 42 additional ordinary parents, source binding and independent teardown verified.')
