"""Verify disjoint native cleanup tests, exact inputs and independent teardown."""
from pathlib import Path
import hashlib,json,re,subprocess,sys,zipfile
r=Path(__file__).resolve().parent;label=sys.argv[1];assert label in ['a','b'];e=r/('batch-'+label)
h=lambda p:hashlib.file_digest(open(p,'rb'),'sha256').hexdigest()
s=json.loads((r/'SOURCE_INPUT.json').read_text());b=json.loads((r/'NATIVE_TEST_INPUT.json').read_text());n=json.loads((e/'APPLICATION_INPUT.json').read_text())
assert n['changed_product_sources']==s['files'] and n['source_revision']==s['reference_commit']
for p,w in s['files'].items():assert h(p)==w,p
for a in b['artifacts']:assert h(a['executable'])==a['sha256']
a=next(x for x in b['artifacts'] if x['crate']=='ottrin_platform');assert (n['source_test_sha256'],n['source_bytes'])==(a['sha256'],a['bytes'])
assert h(e/'actor.ps1')==n['actor_sha256'];assert h(e/'SYSTEM_COMPANION_SOURCE.ps1')==n['system_companion_sha256']
archive=e/'RESULTS.zip'
with zipfile.ZipFile(archive) as z:
    members=sorted(z.namelist());assert {'OWNER.json','RESULT.json','SYSTEM_RESULT.json','SYSTEM_PROCESS.json','CALLER_READY.json','DESKTOP_READY.json'}<=set(members)
    assert set(members)<={'OWNER.json','RESULT.json','SYSTEM_RESULT.json','SYSTEM_PROCESS.json','CALLER_READY.json','DESKTOP_READY.json','OWNER_OBSERVATION.json'}
    records={Path(name).stem:json.loads(z.read(name).decode('utf-8-sig')) for name in members}
for name,v in records.items():(e/('GUEST_OWNER.json' if name=='OWNER' else name+'.json')).write_text(json.dumps(v,indent=2)+'\n')
g=json.loads((e/'GUEST_RESULT.json').read_text());assert g==records['RESULT'] and g['status'].startswith('pass')
system=records['SYSTEM_RESULT'];assert system['status'].startswith('pass') and g['system']==system
expected=json.loads((r/'BATCH_PLAN.json').read_text())['batches'][label];runs=system['runs'];assert [v['case'] for v in runs]==expected
for run in runs:
    assert run['exit']==0 and run['timed_out'] is False,run
    assert re.findall(r'test result: ok\. (\d+) passed; 0 failed; (\d+) ignored',run['stdout'])[-1]==('1','0'),run
    prefix=f"test copy_transaction::windows::journal::tests::cleanup::{run['case']} ... "
    # Abrupt-exit children print between the parent's name and its final `ok`.
    assert re.search(re.escape(prefix)+r"(?:ok|[\s\S]*\nok)\n\ntest result: ok\. 1 passed; 0 failed; 0 ignored",run['stdout']),run
o=json.loads((e/'OWNER.json').read_text());assert all(o['cleanup'][k] is True for k in ['domain_removed','fixture_removed','base_still_off']);assert o['cleanup']['retained_pending_volume'] is None
for pid in [o['private_console']['pid'],o.get('desktop_observer_pid')]:
    if pid:assert not Path('/proc',str(pid)).exists(),pid
assert not Path(o['private_console']['home']).parent.exists()
domains=subprocess.check_output(['virsh','-c','qemu:///system','list','--all'],text=True);volumes=subprocess.check_output(['virsh','-c','qemu:///system','vol-list','default'],text=True)
assert o['domain'] not in domains and o['domain'] not in volumes
v={'status':'pass disjoint native cleanup batch '+label,'tests':expected,'test_passes':len(runs),'native_invocations':len(runs),'archive_sha256':h(archive),'archive_members':members,'source':'../SOURCE_INPUT.json','build':'../NATIVE_TEST_INPUT.json','actor_sha256':n['actor_sha256'],'system_companion_sha256':n['system_companion_sha256'],'independent_teardown':{'domain_volumes_console_observer_profile_absent':True,'domains':domains,'volumes':volumes},'limits':'Ordinary cleanup under SYSTEM; caller payload component has its separate focused receipt. No caller cohort/metadata/UAC/dispatcher/package acceptance.'}
(e/'VERIFICATION.json').write_text(json.dumps(v,indent=2)+'\n');print('PASS',len(runs),'native tests/invocations; source/build/actor binding and independent teardown verified.')
