"""Old accepted index protocol -> current source, in an owned private profile."""
from pathlib import Path
import importlib.util,json,hashlib,tarfile,time,socket,struct
r=Path.cwd();e=Path(__file__).resolve().parent;out=e/'upgrade';assert not out.exists()
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');dsk=importlib.util.module_from_spec(spec);spec.loader.exec_module(dsk)
report={'scope':'Accepted portable protocol1 daemon replaced by current source headless ensure; private profile only'}
def rpc(path,version):
 with socket.socket(socket.AF_UNIX) as c:
  c.settimeout(5);c.connect(str(path));payload=json.dumps({'version':version,'request':{'type':'ping'}}).encode();c.sendall(struct.pack('>I',len(payload))+payload);header=c.recv(4);assert len(header)==4;length=struct.unpack('>I',header)[0];body=b''
  while len(body)<length:
   block=c.recv(length-len(body));assert block;body+=block
  return json.loads(body)
try:
 with dsk.OwnedDesktop(out) as d:
  folder=d.profile/'fixture';folder.mkdir();(folder/'note.txt').write_text('unchanged fixture\n')
  config=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];config['search'].update(include_roots=[str(folder)],content_indexing_enabled=False)
  cfg=d.profile/'config/ottrin/config.json';cfg.parent.mkdir();cfg.write_text(json.dumps(config)+'\n');before=hashlib.sha256(cfg.read_bytes()).hexdigest()
  olddir=d.profile/'previous';olddir.mkdir();old=olddir/'ottrin-indexd'
  with tarfile.open(r/'dist/packages/Ottrin-0.1.0-linux-x86_64-portable.tar.gz') as archive:
   member=archive.getmember('Ottrin-0.1.0-linux-x86_64/libexec/ottrin/ottrin-indexd');old.write_bytes(archive.extractfile(member).read())
  old.chmod(0o700);report['old_binary_sha256']=hashlib.sha256(old.read_bytes()).hexdigest()
  previous=d.spawn([str(old),'--index-daemon'],'previous.log');endpoint=d.profile/'runtime/ottrin/index-v1.sock';deadline=time.monotonic()+30
  while not endpoint.exists():assert previous.poll() is None and time.monotonic()<deadline;time.sleep(.05)
  report['before']=rpc(endpoint,1);assert report['before']['response']['type']=='pong'
  product=r/'target/debug/ottrin-app';report['new_binary_sha256']=hashlib.sha256(product.read_bytes()).hexdigest()
  result=dsk.run([str(product),'index','ensure'],d.env,False);report['ensure']={'exit':result.returncode,'stdout':result.stdout,'stderr':result.stderr};assert result.returncode==0,report['ensure']
  previous.wait(timeout=5);report['previous_exit']=previous.returncode
  report['after']=rpc(endpoint,2);assert report['after']['response']['type']=='pong'
  assert hashlib.sha256(cfg.read_bytes()).hexdigest()==before;assert (folder/'note.txt').read_text()=='unchanged fixture\n'
  report.update(status='pass',preferences_preserved=True,source_preserved=True)
finally:
 report['teardown']=json.loads((out/'TEARDOWN.json').read_text()) if (out/'TEARDOWN.json').exists() else None;(out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
