"""Verify final native corrections and combine explicitly scoped component receipts."""
from pathlib import Path
import hashlib,json,re,subprocess,zipfile
r=Path(__file__).resolve().parent;e=r/'final-native'
h=lambda p:hashlib.file_digest(open(p,'rb'),'sha256').hexdigest()
s=json.loads((r/'FIXED_SOURCE_INPUT.json').read_text());old=json.loads((r/'SOURCE_INPUT.json').read_text());b=json.loads((r/'FIXED_NATIVE_TEST_INPUT.json').read_text());n=json.loads((e/'APPLICATION_INPUT.json').read_text())
assert n['changed_product_sources']==s['files'] and n['source_revision']==s['reference_commit']
for p,w in s['files'].items():assert h(p)==w,p
changed=[p for p,w in old['files'].items() if s['files'][p]!=w];assert changed==s['fixture_only_differences'];assert len(changed)==2 and all('/tests/' in p for p in changed)
for a in b['artifacts']:assert h(a['executable'])==a['sha256']
a=next(x for x in b['artifacts'] if x['crate']=='ottrin_platform');assert (n['source_test_sha256'],n['source_bytes'])==(a['sha256'],a['bytes'])
assert h(e/'actor.ps1')==n['actor_sha256'] and h(e/'SYSTEM_COMPANION_SOURCE.ps1')==n['system_companion_sha256']
archive=e/'RESULTS.zip'
with zipfile.ZipFile(archive) as z:
    members=sorted(z.namelist());assert set(members)=={'OWNER.json','RESULT.json','SYSTEM_RESULT.json','SYSTEM_PROCESS.json','CALLER_READY.json','DESKTOP_READY.json'}
    records={Path(name).stem:json.loads(z.read(name).decode('utf-8-sig')) for name in members}
for name,v in records.items():(e/('GUEST_OWNER.json' if name=='OWNER' else name+'.json')).write_text(json.dumps(v,indent=2)+'\n')
g=json.loads((e/'GUEST_RESULT.json').read_text());system=records['SYSTEM_RESULT'];assert g==records['RESULT'] and g['status'].startswith('pass') and system['status'].startswith('pass') and g['system']==system
plan=json.loads((r/'FINAL_TEST_PLAN.json').read_text())['tests'];runs=system['runs'];assert len(runs)==3
for run,test in zip(runs,plan,strict=True):
    assert run['case']==test['case'] and run['exit']==0 and run['timed_out'] is False
    assert re.findall(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored',run['stdout'])[-1]==('1','0')
    assert re.search(re.escape('test '+test['qualified_test']+' ... ')+r'(?:ok|[\s\S]*\nok)\n\ntest result: ok\. 1 passed; 0 failed; 0 ignored',run['stdout'])
o=json.loads((e/'OWNER.json').read_text());assert all(o['cleanup'][k] is True for k in ['domain_removed','fixture_removed','base_still_off']);assert o['cleanup']['retained_pending_volume'] is None
for pid in [o['private_console']['pid'],o.get('desktop_observer_pid')]:
    if pid:assert not Path('/proc',str(pid)).exists(),pid
assert not Path(o['private_console']['home']).parent.exists()
domains=subprocess.check_output(['virsh','-c','qemu:///system','list','--all'],text=True);volumes=subprocess.check_output(['virsh','-c','qemu:///system','vol-list','default'],text=True)
assert o['domain'] not in domains and o['domain'] not in volumes
final={'status':'pass all three final native cases','archive_sha256':h(archive),'archive_members':members,'source':'../FIXED_SOURCE_INPUT.json','build':'../FIXED_NATIVE_TEST_INPUT.json','tests':plan,'test_passes':3,'native_invocations':3,'independent_teardown':{'domain_volumes_console_observer_profile_absent':True,'domains':domains,'volumes':volumes}}
(e/'VERIFICATION.json').write_text(json.dumps(final,indent=2)+'\n')
a=json.loads((r/'batch-a/VERIFICATION.json').read_text());bb=json.loads((r/'batch-b/VERIFICATION.json').read_text());focused=json.loads((r/'FOCUSED_VERIFICATION.json').read_text());assert (a['test_passes'],bb['test_passes'],focused['top_level_passes'])==(46,43,108)
catalog=json.loads((r/'CLEANUP_TEST_CATALOG.json').read_text())['tests'];expected={x['name'] for x in catalog if not x['ignored']};assert len(expected)==91
selected=a['tests']+bb['tests']+[t['qualified_test'].split('::')[-1] for t in plan if '::cleanup::' in t['qualified_test']];assert len(selected)==len(set(selected))==91 and set(selected)==expected
assert (r/'FIXTURE_CORRECTIONS.patch').is_file()
limits=['Caller retained-payload cleanup component only; caller cohort/metadata retirement and public cleanup entry points remain unconverted; Administrator Copy stays disabled','Same authenticated live-client context recaptures do not prove real app/helper restart, newly authenticated connections or alternate-administrator UAC','Full dispatcher, complete caller failure/readonly/link/alias matrices, TokenOwner mutation and packaged profile contexts remain required','Stable application packages unchanged; hosted acceptance of the new checkpoint remains separate']
value={'status':'pass caller retained payload cleanup and complete ordinary cleanup regression coverage with corrected fixtures','test_passes':200,'native_invocations':123,'cleanup_nonworker_tests':91,'sources':{'initial':'SOURCE_INPUT.json','final':'FIXED_SOURCE_INPUT.json','only_post_bulk_changes':changed,'corrections':'FIXTURE_CORRECTIONS.patch','production_files_unchanged':True},'receipts':{'focused':'FOCUSED_VERIFICATION.json','bulk_a':'batch-a/VERIFICATION.json','bulk_b':'batch-b/VERIFICATION.json','final':'final-native/VERIFICATION.json'},'scope':'108 focused passes plus 89 ordinary cleanup cases on initial build; two fixture-only corrections and final three exact native cases on rebuilt source. Covers all 91 non-worker cleanup tests; four worker entry points intentionally excluded from parent invocation. This is component evidence, not complete caller cleanup or whole-roadmap acceptance.','lint':{'command':'cargo clippy -p ottrin-platform -p ottrin-ui --target x86_64-pc-windows-gnu --all-targets -- -D warnings','exit':0,'sha256':h(r/'fixed-windows-lint.log')},'limits':limits}
(r/'ACCEPTANCE.json').write_text(json.dumps(value,indent=2)+'\n');(r/'FIXED_BUILD_BINDING.json').write_text(json.dumps({'status':'pass final source/build/staged executable/native result binding','source':'FIXED_SOURCE_INPUT.json','build':'FIXED_NATIVE_TEST_INPUT.json','native':'final-native/APPLICATION_INPUT.json','unchanged_executable_text':n['unchanged_executable_text'],'result':'final-native/VERIFICATION.json'},indent=2)+'\n')
print('PASS 200 test passes across 123 invocations; all 91 cleanup cases covered; exact final corrections and independent teardown verified.')
