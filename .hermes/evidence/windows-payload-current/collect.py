from pathlib import Path
import sys,base64,json
sys.path.insert(0,str(Path('.hermes/evidence/windows-20261011').resolve()))
from guest_exec import qga
out=Path('.hermes/evidence/windows-payload-current')
def pull(source,target):
 h=qga('guest-file-open',{'path':source,'mode':'rb'})
 try:
  data=bytearray()
  while True:
   r=qga('guest-file-read',{'handle':h,'count':65536});data.extend(base64.b64decode(r.get('buf-b64','')))
   if r.get('eof'):break
 finally:qga('guest-file-close',{'handle':h})
 if target.suffix=='.json':target.write_text(json.dumps(json.loads(data.decode('utf-8-sig')),indent=2)+'\n')
 else:target.write_text(data.decode('utf-16' if data[:2] in [b'\xff\xfe',b'\xfe\xff'] else 'utf-16le' if target.name in ['phase-debug.log','initial-wrapper-rejected.log'] else 'utf-8-sig').replace('\r\n','\n'))
for name in ['RESULTS.json','LEAF.json','PROGRESS.json']:
 pull('C:\\OttrinQA-20261011\\payload-current\\'+name,out/name)
pull('C:\\OttrinQA-20261011\\payload-current\\phase-debug.txt',out/'phase-debug.log')
pull('C:\\OttrinQA-20261011\\phase-debug.txt',out/'initial-wrapper-rejected.log')
r=json.loads((out/'RESULTS.json').read_text());assert r['status']=='pass';assert {c['case'] for c in r['cases']}=={'Foreign','Staging','Service','Commit','Lock','Stale','Success','PendingUninstall'}
assert json.loads((out/'LEAF.json').read_text())['status']=='pass'
print('Collected current eight-case native acceptance and guarded leaf retirement')
