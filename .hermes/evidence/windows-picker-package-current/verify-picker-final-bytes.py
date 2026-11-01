"""Read final private fixture bytes independently through QGA after the GUI closes."""
from pathlib import Path, PureWindowsPath
import base64, hashlib, json
import native_compiler as n
p=Path(__file__).resolve().parent
e=p.parent/'windows-picker-gui-current'
session=json.loads((e/'SESSION.json').read_text())
assert session['status']=='waiting for owned input'
assert session['guest_root']==r'C:\OttrinWindowQA'
expected_path=p/'EXPECTED_FIXTURE_TEXT.json'
expected=json.loads(expected_path.read_text())
assert expected and all(isinstance(k,str) and isinstance(v,str) for k,v in expected.items())
checks={}
for name,text in expected.items():
 relative=PureWindowsPath(name)
 assert not relative.is_absolute() and not relative.drive and '..' not in relative.parts
 wanted=text.encode('utf-8')
 assert len(wanted)<1024*1024
 path=str(PureWindowsPath(session['guest_root'])/'cases'/'ordinary'/'fixture'/relative)
 handle=n.qga('guest-file-open',{'path':path,'mode':'rb'})
 actual=bytearray()
 try:
  while True:
   block=n.qga('guest-file-read',{'handle':handle,'count':65536})
   actual.extend(base64.b64decode(block.get('buf-b64','')))
   assert len(actual)<=len(wanted),'Unexpected appended fixture bytes: '+name
   if block.get('eof'):break
 finally:n.qga('guest-file-close',{'handle':handle})
 assert actual==wanted,'Raw fixture bytes changed: '+name
 checks[name]={'bytes':len(actual),'sha256':hashlib.sha256(actual).hexdigest(),'matches_independently_declared_utf8_bytes':True}
result={'status':'pass','source':json.loads((p/'BUILD_SOURCE.json').read_text())['source_revision'],'nonce':session['nonce'],'expected_manifest_sha256':hashlib.sha256(expected_path.read_bytes()).hexdigest(),'files':checks,'scope':'Independent raw guest-file reads,not text decoding or hash strings supplied to the UI verifier. Run after explicit normal product Close and before owned teardown.'}
(e/'FINAL_RAW_BYTES.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
