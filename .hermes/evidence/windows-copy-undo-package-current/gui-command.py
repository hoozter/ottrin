from pathlib import Path
import sys,json,time,tempfile,os
p=Path('.hermes/evidence/windows-copy-undo-gui-current');s=json.loads((p/'SESSION.json').read_text());assert s['status']=='waiting for owned input',s
command=json.loads(sys.argv[1]);sequence=s['next_sequence'];request={'nonce':s['nonce'],'sequence':sequence,'op':'finish' if command['op']=='finish' else 'action','command':command}
# Publish a complete request atomically, without replacing an outstanding one.
with tempfile.NamedTemporaryFile(mode='w', prefix='.request-', dir=p, delete=False) as f:
 json.dump(request,f); temporary=Path(f.name)
try:os.link(temporary,p/'NEXT.json')
finally:temporary.unlink()
deadline=time.monotonic()+100
while time.monotonic()<deadline:
 try:a=json.loads((p/'SESSION.json').read_text())
 except json.JSONDecodeError:time.sleep(.1);continue
 if a['status']!='waiting for owned input' or a['next_sequence']!=sequence:
  print(json.dumps(a,indent=2))
  if a['status']=='waiting for owned input':print((p/'desktop.json').read_text(encoding='utf-8-sig'))
  elif command['op']!='finish' or a['status'] not in ('requested completion; teardown pending','pass'):raise RuntimeError('Owned controller failed; inspect its receipt')
  break
 time.sleep(.2)
else:raise TimeoutError('Observe the same owned GUI command; do not dispatch it again')
