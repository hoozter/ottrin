"""Verify exact native component results and independent teardown, preserving compact receipts."""
from pathlib import Path
import hashlib,json,re,subprocess,zipfile,os
r=Path(__file__).resolve().parent
h=lambda p:hashlib.file_digest(open(p,'rb'),'sha256').hexdigest()
def save(name,value): (r/name).write_text(json.dumps(value,indent=2)+'\n')
s=json.loads((r/'SOURCE_INPUT.json').read_text()); b=json.loads((r/'NATIVE_TEST_INPUT.json').read_text());n=json.loads((r/'native/APPLICATION_INPUT.json').read_text())
assert n['source_revision']==s['reference_commit'] and n['changed_product_sources']==s['files']
for p,digest in s['files'].items(): assert h(p)==digest,p
for a in b['artifacts']: assert h(a['executable'])==a['sha256']
a=next(x for x in b['artifacts'] if x['crate']=='ottrin_platform')
assert (n['source_test_sha256'],n['source_bytes'])==(a['sha256'],a['bytes'])
for name,key in [('actor.ps1','actor_sha256'),('system-companion.ps1','system_companion_sha256')]:assert h(r/name)==n[key]
archive=r/'native/RESULTS.zip'
with zipfile.ZipFile(archive) as z:
 members=sorted(z.namelist());assert members==sorted(['CALLER_READY.json','DESKTOP_READY.json','OWNER.json','OWNER_OBSERVATION.json','RESULT.json','SYSTEM_PROCESS.json','SYSTEM_RESULT.json'])
 result={Path(name).stem:json.loads(z.read(name).decode('utf-8-sig')) for name in members}
 for name,value in result.items(): (r/'native'/('GUEST_OWNER.json' if name=='OWNER' else name+'.json')).write_text(json.dumps(value,indent=2)+'\n')
g=json.loads((r/'native/GUEST_RESULT.json').read_text());assert result['RESULT']==g
assert g['status'].startswith('pass') and result['SYSTEM_RESULT']['status'].startswith('pass')
runs=g['runs']+result['SYSTEM_RESULT']['runs'];assert len(runs)==43
count=0
for run in runs:
 assert run['exit']==0,run
 matches=re.findall(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored',run['stdout']);assert matches,run
 assert matches[-1][1]==('1' if run['case']=='native Copy security regressions' else '0'),run
 if run['case']=='native Copy security regressions':assert 'new_copy_candidate_inherits_the_real_parent_audit_sacl ... ignored, requires SeSecurityPrivilege' in run['stdout']
 assert not run.get('timed_out',False),run
 count+=int(matches[-1][0])
assert count==120,count
helper=next(x for x in runs if x['case']=='caller private helper')
for directory in ['false','true']:assert f'caller journal source_directory={directory}: staging, reopen, plan and three Begin recovery cases passed' in helper['stdout']
for directory in ['false','true']:assert f'caller receipt source_directory={directory}: unified execution, canonical receipt refresh, public Undo and idempotent replay passed' in helper['stdout']
for directory in ['false','true']:
 for case in range(4):assert f'caller resume source_directory={directory} case={case}: fresh admission, cancellation preservation, completion, readonly replay and public Undo passed' in helper['stdout']
for directory in ['false','true']:
 for case in range(2):assert f'caller overwrite source_directory={directory} case={case}: private original retention, publication, receipt refresh, public Undo and exact restoration passed' in helper['stdout']
for directory in ['false','true']:assert f'caller fresh recovery source_directory={directory}: recaptured-context discovery, restored Resume, restored Undo, cancellation and readonly evidence passed' in helper['stdout']
for directory in ['false','true']:
 for undo in ['false','true']:
  for kind,detail in [('metadata','paired intent, read-only fresh admission, private authority, ordered removal and idempotent replay'),('cleanup','retained payload deletion, cancellation, private authority and public data preservation')]:
   assert f'caller {kind} source_directory={directory} undo={undo}: {detail} passed' in helper['stdout']
plan=json.loads((r/'NATIVE_PLAN.json').read_text())
metadata_runs=[x for x in result['SYSTEM_RESULT']['runs'] if x['case'].startswith('metadata regression ')]
assert [x['case'].removeprefix('metadata regression ') for x in metadata_runs]==plan['ordinary_metadata_cases']
for run in metadata_runs:
 name=run['case'].removeprefix('metadata regression ')
 prefix='test copy_transaction::windows::journal::tests::cleanup::'+name+' ... '
 assert re.search(re.escape(prefix)+r'(?:ok|[\s\S]*\nok)\n\ntest result: ok\. 1 passed; 0 failed; 0 ignored',run['stdout']),name
for directory in ['false','true']:
 for interrupted in ['false','true']:
  assert f'caller cohort source_directory={directory} interrupted={interrupted}: mixed payloads, canonical fresh admission, cancellation, private log lifecycle, ordered metadata recovery and public preservation passed' in helper['stdout']
owner=json.loads((r/'native/OWNER.json').read_text());assert all(owner['cleanup'][k] is True for k in ['domain_removed','fixture_removed','base_still_off']); assert owner['cleanup']['retained_pending_volume'] is None
print('cleanup fields:',owner['cleanup'])
domains=subprocess.check_output(['virsh','-c','qemu:///system','list','--all'],text=True)
volumes=subprocess.check_output(['virsh','-c','qemu:///system','vol-list','default'],text=True)
assert 'ottrin-boundary-client' not in domains and 'ottrin-boundary-client' not in volumes
for key in ['pid','desktop_observer_pid']:
 pid=owner.get(key) if key!='pid' else owner.get('private_console',{}).get('pid')
 if pid: assert not Path('/proc',str(pid)).exists(),(key,pid)
private=owner['private_console'];assert not Path(private['home']).parent.exists()
limits=['Mixed caller cohort component only; additional ordinary group/cohort regressions, caller shared aliases and public cleanup integration remain incomplete; Administrator Copy stays disabled','Fresh contexts of the same live authenticated client do not prove actual client/helper restart, new authenticated connections or alternate-administrator UAC','Full dispatcher, complete caller prefix/readonly/link/alias failure matrices, TokenOwner mutation and packaged profile contexts remain required','Stable application packages unchanged; hosted acceptance remains separate']
v={'status':'pass caller mixed cleanup cohorts and affected ordinary metadata/cohort regressions','archive_sha256':h(archive),'archive_members':members,'native_invocations':len(runs),'top_level_passes':count,'caller_metadata_scenarios':4,'caller_mixed_cohort_scenarios':4,'ordinary_metadata_regressions':plan['ordinary_metadata_cases'],'compiler_jsonl_sha256':b['compiler_jsonl_sha256'],'independent_teardown':{'domains':domains,'volumes':volumes,'console_observer_profile_absent':True},'limits':limits}
save('FOCUSED_VERIFICATION.json',v)
save('BUILD_BINDING.json',{'status':'pass exact source/build/staged executable and native runtime binding','source':'SOURCE_INPUT.json','build':'NATIVE_TEST_INPUT.json','native':'native/APPLICATION_INPUT.json','source_files':len(s['files']),'unchanged_executable_text':n['unchanged_executable_text'],'native_result':v['status'],'remaining':'42 additional ordinary group/cohort regressions and caller shared-alias/public integration remain separate'})
print(f'PASS {count} focused native tests across {len(runs)} invocations; four caller mixed cohorts and independent teardown verified. Additional 42 group/cohort regressions remain separate.')
