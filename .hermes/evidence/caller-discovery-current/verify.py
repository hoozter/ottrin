"""Verify exact native component results and independent teardown, preserving compact receipts."""
from pathlib import Path
import hashlib,json,re,subprocess,zipfile,os
r=Path(__file__).resolve().parent
h=lambda p:hashlib.file_digest(open(p,'rb'),'sha256').hexdigest()
def save(name,value): (r/name).write_text(json.dumps(value,indent=2)+'\n')
s=json.loads((r/'SOURCE_INPUT.json').read_text()); b=json.loads((r/'NATIVE_TEST_INPUT.json').read_text());n=json.loads((r/'native-fixed/APPLICATION_INPUT.json').read_text())
assert n['source_revision']==s['reference_commit'] and n['changed_product_sources']==s['files']
for p,digest in s['files'].items(): assert h(p)==digest,p
for a in b['artifacts']: assert h(a['executable'])==a['sha256']
a=next(x for x in b['artifacts'] if x['crate']=='ottrin_platform')
assert (n['source_test_sha256'],n['source_bytes'])==(a['sha256'],a['bytes'])
for name,key in [('actor.ps1','actor_sha256'),('system-companion.ps1','system_companion_sha256')]:assert h(r/name)==n[key]
archive=r/'native-fixed/RESULTS.zip'
with zipfile.ZipFile(archive) as z:
 members=sorted(z.namelist());assert members==sorted(['CALLER_READY.json','DESKTOP_READY.json','OWNER.json','OWNER_OBSERVATION.json','RESULT.json','SYSTEM_PROCESS.json','SYSTEM_RESULT.json'])
 result={Path(name).stem:json.loads(z.read(name).decode('utf-8-sig')) for name in members}
 for name,value in result.items(): (r/'native-fixed'/('GUEST_OWNER.json' if name=='OWNER' else name+'.json')).write_text(json.dumps(value,indent=2)+'\n')
g=json.loads((r/'native-fixed/GUEST_RESULT.json').read_text());assert result['RESULT']==g
assert g['status'].startswith('pass') and result['SYSTEM_RESULT']['status'].startswith('pass')
runs=g['runs']+result['SYSTEM_RESULT']['runs'];assert len(runs)==31
count=0
for run in runs:
 assert run['exit']==0,run
 match=re.search(r'test result: ok\. (\d+) passed; 0 failed',run['stdout']);assert match,run
 count+=int(match[1])
assert count==108,count
helper=next(x for x in runs if x['case']=='caller private helper')
for directory in ['false','true']:assert f'caller journal source_directory={directory}: staging, reopen, plan and three Begin recovery cases passed' in helper['stdout']
for directory in ['false','true']:assert f'caller receipt source_directory={directory}: unified execution, canonical receipt refresh, public Undo and idempotent replay passed' in helper['stdout']
for directory in ['false','true']:
 for case in range(4):assert f'caller resume source_directory={directory} case={case}: fresh admission, cancellation preservation, completion, readonly replay and public Undo passed' in helper['stdout']
for directory in ['false','true']:
 for case in range(2):assert f'caller overwrite source_directory={directory} case={case}: private original retention, publication, receipt refresh, public Undo and exact restoration passed' in helper['stdout']
for directory in ['false','true']:assert f'caller fresh recovery source_directory={directory}: recaptured-context discovery, restored Resume, restored Undo, cancellation and readonly evidence passed' in helper['stdout']
owner=json.loads((r/'native-fixed/OWNER.json').read_text());assert all(owner['cleanup'][k] is True for k in ['domain_removed','fixture_removed','base_still_off']); assert owner['cleanup']['retained_pending_volume'] is None
print('cleanup fields:',owner['cleanup'])
domains=subprocess.check_output(['virsh','-c','qemu:///system','list','--all'],text=True)
volumes=subprocess.check_output(['virsh','-c','qemu:///system','vol-list','default'],text=True)
assert 'ottrin-boundary-client' not in domains and 'ottrin-boundary-client' not in volumes
for key in ['pid','desktop_observer_pid']:
 pid=owner.get(key) if key!='pid' else owner.get('private_console',{}).get('pid')
 if pid: assert not Path('/proc',str(pid)).exists(),(key,pid)
private=owner['private_console'];assert not Path(private['home']).parent.exists()
limits=['Complete caller mutation-prefix coverage, readonly original-root retention and readonly/link publication recovery remain separate native gates','Fresh captures of the same authenticated client only; real client/helper restart, new authenticated connections/UAC, full dispatcher, all prefix/readonly/link cases and caller cleanup remain incomplete','Caller cleanup records may remain NeedsReview until their own observers and mutations carry caller authority; complete original failure matrix, real restart recovery, alternate-admin UAC, links, TokenOwner mutation and package/profile contexts remain separate gates','Administrator Copy stays disabled; stable packages unchanged']
v={'status':'pass admitted caller registry discovery and restored Resume/Undo across fresh context captures','archive_sha256':h(archive),'archive_members':members,'native_invocations':len(runs),'top_level_passes':count,'compiler_jsonl_sha256':b['compiler_jsonl_sha256'],'independent_teardown':{'domains':domains,'volumes':volumes,'console_observer_profile_absent':True},'limits':limits}
save('VERIFICATION.json',v)
save('BUILD_BINDING.json',{'status':'pass exact source/build/staged executable and native runtime binding','source':'SOURCE_INPUT.json','build':'NATIVE_TEST_INPUT.json','native':'native-fixed/APPLICATION_INPUT.json','source_files':len(s['files']),'unchanged_executable_text':n['unchanged_executable_text'],'native_result':v['status']})
save('ACCEPTANCE.json',{'status':v['status'],'source':'SOURCE_INPUT.json','build':'NATIVE_TEST_INPUT.json','binding':'BUILD_BINDING.json','native':'VERIFICATION.json','top_level_passes':count,'native_invocations':len(runs),'coverage':['Admitted registry objects expose discovery and restored Resume/Undo admissions using retained authority; shared ordinary/caller directory inspection validates record creators, native parent IDs and paired payload evidence without granting by presentation rows','Actual file and four-entry directory cases recapture independent caller contexts after staging and again after completion, discover correct phases/evidence, restore and execute Resume and Undo, reject invalid IDs and cancellation, preserve log bytes during discovery and preserve source/user-state evidence; prior overwrite/recovery/receipt controls remain','Ordinary source-free publication/permission replay and receipt/public Undo still pass with shared retained verifier','Prior caller paired-journal, Begin recovery, claim, profile, token, metadata, tree/security and codec regressions pass'],'lint':{'command':'cargo clippy -p ottrin-platform -p ottrin-ui --target x86_64-pc-windows-gnu --all-targets -- -D warnings','exit':0,'sha256':h(r/'windows-lint.log')},'limits':limits})
print(f'PASS {count} native tests across {len(runs)} invocations; independent teardown verified.')
