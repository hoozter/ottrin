"""Exercise the exact packaged Linux helper's read-only stdio boundary in private state."""
from pathlib import Path
import hashlib,importlib.util,json,signal,subprocess,sys
sys.dont_write_bytecode=True
R=Path.cwd();E=Path(__file__).resolve().parent
s=importlib.util.spec_from_file_location('workspace',R/'scripts/test-workspace.py');w=importlib.util.module_from_spec(s);s.loader.exec_module(w)
for sig in [signal.SIGINT,signal.SIGTERM]:signal.signal(sig,w.cancelled)
binary=R/'target/release/ottrin-priv-helper';content=json.loads((E/'PACKAGE_CONTENT_ACCEPTANCE.json').read_text());digest=hashlib.sha256(binary.read_bytes()).hexdigest();assert digest==content['runtime_sha256']['privileged_helper'];results=[]
with w.isolated_environment(prefix='ot-helper-wire-') as (profile,env):
 corpus=profile/'corpus';corpus.mkdir();(corpus/'visible.txt').write_text('visible fixture\n');(corpus/'.hidden').write_text('hidden fixture\n');before={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in corpus.iterdir()}
 for version,hidden in [(3,False),(3,True),(2,True)]:
  request={'protocol_version':version,'command':{'ListDirectory':{'path':str(corpus),'show_hidden':hidden}},'context':{'reason':'Owned read-only wire acceptance','cwd':str(corpus)}}
  child=subprocess.Popen([str(binary),'--stdio-json'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=env,start_new_session=True)
  try:out,err=child.communicate(json.dumps(request).encode(),timeout=10);assert child.returncode==0 and not err
  finally:w.stop_tests(child)
  reply=json.loads(out)
  if version==3:
   assert reply['status']=='Success';names=sorted(v['name'] for v in reply['payload']['Entries']);assert names==(['.hidden','visible.txt'] if hidden else ['visible.txt'])
  else:assert reply['status']=='Failed' and reply['payload'] is None and 'protocol does not match' in reply['message']
  results.append({'version':version,'show_hidden':hidden,'response':reply})
 assert before=={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in corpus.iterdir()}
assert not profile.exists()
(E/'LINUX_HELPER_STDIO.json').write_text(json.dumps({'status':'pass current Linux helper stdio read-only boundary','source_revision':content['source_revision'],'binary_sha256':digest,'cases':results,'fixture_bytes_unchanged':True,'owned_profile_removed':True,'scope':'Exact current packaged helper bytes; ordinary user, read-only directory listing, hidden-file semantics and mismatched-protocol refusal. No elevated Copy or system setup acceptance.'},indent=2)+'\n')
print('Current helper stdio and protocol refusal passed; private fixture removed')
