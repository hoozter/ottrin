"""Actual helper IPC with disposable user state; no elevation/host profile access."""
from pathlib import Path
import hashlib,importlib.util,json,os,subprocess,sys
sys.dont_write_bytecode=True
r=Path.cwd();e=Path(__file__).resolve().parent;binary=r/'target/debug/ottrin-priv-helper'
assert os.geteuid()!=0
spec=importlib.util.spec_from_file_location('workspace',r/'scripts/test-workspace.py');workspace=importlib.util.module_from_spec(spec);spec.loader.exec_module(workspace)
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
report={'status':'running','helper_sha256':sha(binary),'scope':'Actual current debug helper stdio JSON under ordinary user and disposable HOME/XDG; not pkexec or packaged/local-seat acceptance','cases':[]}
with workspace.isolated_environment() as (profile,env):
    source=profile/'input';destination=profile/'output';source.mkdir();destination.mkdir()
    (source/'a').write_text('new a');(source/'b').write_text('new b');(destination/'a').write_text('old a')
    def request(command):
        result=subprocess.run([str(binary),'--stdio-json'],input=json.dumps({'protocol_version':2,'command':command,'context':{'reason':'Owned stdio acceptance','cwd':str(source)}}),text=True,capture_output=True,env=env,timeout=20)
        assert result.returncode==0 and not result.stderr,result
        return json.loads(result.stdout)
    copied=request({'File':{'Copy':{'sources':[str(source/'a'),str(source/'b')],'destination':str(destination),'conflict':'Overwrite'}}})
    assert copied['status']=='Success',copied
    references=copied['payload']['Copy'];ids=references['transactions']
    def snapshot():
        return {str(p.relative_to(profile)):sha(p) for p in profile.rglob('*') if p.is_file()}
    def history():
        before=snapshot()
        result=request('DiscoverCopyHistory')
        assert result['status']=='Success',result
        assert before==snapshot(), 'History discovery changed file contents or names'
        return result['payload']['CopyHistory']
    observed=history()
    assert len(observed)==1 and observed[0]['reference']['transactions']==ids
    assert observed[0]['undo_unavailable_reason'] is None
    assert request({'DiscoverCopyHistory':{}})['status']=='Failed'
    report['cases'].append('Read-only fresh-process history discovers the complete batch without changing file contents or names')
    assert references['completed_sources']==2 and len(ids)==len(references['paths'])==2
    assert (destination/'a').read_text()=='new a' and (destination/'b').read_text()=='new b'
    assert request({'UndoCopy':{'transactions':ids}})['status']=='Success'
    assert (destination/'a').read_text()=='old a' and not (destination/'b').exists()
    assert history()[0]['undo_unavailable_reason'] is not None
    assert request({'CleanupCopy':{'transactions':ids}})['status']=='Success'
    assert history()==[]
    assert not list(destination.glob('.ottrin-txn-*'))
    report['cases'].append('Fresh helper processes: exact Copy JSON returns durable references, Undo restores overwritten original/removes new publication, explicit cleanup removes stages')
    copied=request({'File':{'Copy':{'sources':[str(source/'a')],'destination':str(destination),'conflict':'Overwrite'}}});assert copied['status']=='Success'
    ids=copied['payload']['Copy']['transactions'];(destination/'a').write_text('user changed')
    refused=request({'UndoCopy':{'transactions':ids}});assert refused['status']=='Failed' and (destination/'a').read_text()=='user changed'
    report['cases'].append('Changed publication refuses Undo without mutation')
    assert history()[0]['undo_unavailable_reason'] is not None
    assert request({'CleanupCopy':{'transactions':ids}})['status']=='Success'
    assert history()==[]
    assert (destination/'a').read_text()=='user changed' and not list(destination.glob('.ottrin-txn-*'))
    report['cases'].append('Explicit cleanup preserves changed publication')
    for command in ['UndoCopy','CleanupCopy']:
        for ids in [[],['../outside'],['0'*32]]:
            assert request({command:{'transactions':ids}})['status']=='Failed'
    report['cases'].append('Malformed, empty and unknown references refused across both IPC actions')
    assert (source/'a').read_text()=='new a' and (source/'b').read_text()=='new b'
    assert not list((profile/'state/ottrin/copies').glob('*.journal'))
    report['source_preserved']=True
report.update(status='pass fresh-process Copy history discovery and guarded lifecycle',owned_profile_removed=not profile.exists())
(e/'STDIO_ACCEPTANCE.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report,indent=2))
