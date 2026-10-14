from pathlib import Path
import sys,base64,json
b=Path(__file__).resolve().parent
import guest_exec
guest_exec.DOMAIN='ottrin-index-qa'
name=sys.argv[1]
assert name in ['before.png','after.png','settled.png','Launch.json','Move.json','Finish.json']
h=guest_exec.qga('guest-file-open',{'path':'C:\\OttrinQA-20261011\\move-wake-current\\'+name,'mode':'rb'})
try:
 data=bytearray()
 while True:
  r=guest_exec.qga('guest-file-read',{'handle':h,'count':65536});data.extend(base64.b64decode(r.get('buf-b64','')))
  if r.get('eof'):break
finally:guest_exec.qga('guest-file-close',{'handle':h})
p=b/('windows-move-wake-'+name)
if name.endswith('.json'):p.write_text(json.dumps(json.loads(data.decode('utf-8-sig')),indent=2)+'\n')
else:p.write_bytes(data)
print(str(p))
