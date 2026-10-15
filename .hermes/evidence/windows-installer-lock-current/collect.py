import sys,pathlib,base64,json
sys.path.insert(0,str(pathlib.Path('.hermes/evidence/windows-20261011').resolve()))
from guest_exec import qga
p=pathlib.Path('.hermes/evidence/windows-installer-lock-current/RESULTS.json')
h=qga('guest-file-open',{'path':r'C:\OttrinQA-20261011\installer-lock-current\RESULTS.json','mode':'rb'})
try:
 raw=bytearray()
 while True:
  r=qga('guest-file-read',{'handle':h,'count':65536});raw.extend(base64.b64decode(r.get('buf-b64','')))
  if r.get('eof'):break
finally:qga('guest-file-close',{'handle':h})
v=json.loads(raw.decode('utf-8-sig'));assert v['status']=='pass' and len(v['cases'])==3;p.write_text(json.dumps(v,indent=2)+'\n');print('Collected native before/after and real-uninstaller exclusion results')
