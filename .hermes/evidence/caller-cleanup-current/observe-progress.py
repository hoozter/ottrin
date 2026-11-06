"""Read only the recorded owned guest's current cleanup batch progress."""
from pathlib import Path
import base64, json, subprocess, sys, time
r=Path(__file__).resolve().parent/sys.argv[1]
o=json.loads((r/'OWNER.json').read_text())
if o.get('cleanup',{}).get('domain_removed') is True:
    print(json.dumps({'status':'producer terminal; inspect verification receipt','actor':o.get('actor_observation',{}).get('status')}));sys.exit(0)
uuid=subprocess.check_output(['virsh','-c','qemu:///system','domuuid',o['domain']],text=True).strip()
assert uuid==o['uuid'], 'Owned guest identity changed'
root=json.loads((r/'GUEST_PREPARATION.json').read_text())['root']
code="$p='"+root+"\\cleanup-progress.json';if(Test-Path $p){Get-Content $p -Raw}"
def call(a):
    return json.loads(subprocess.check_output(['virsh','-c','qemu:///system','qemu-agent-command',o['domain'],json.dumps(a)],text=True))['return']
pid=call({'execute':'guest-exec','arguments':{'path':'C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe','arg':['-NoProfile','-NonInteractive','-EncodedCommand',base64.b64encode(code.encode('utf-16le')).decode()],'capture-output':True}})['pid']
for _ in range(20):
    x=call({'execute':'guest-exec-status','arguments':{'pid':pid}})
    if x.get('exited'): break
    time.sleep(.5)
(r/'PROGRESS_OBSERVER_PROCESS.json').write_text(json.dumps({'pid':pid,'result':x},indent=2)+'\n')
if not x.get('exited'):
    print(json.dumps({'observer_pid':pid,'status':'still running; preserve handle'}));sys.exit(0)
assert x.get('exitcode')==0, x
raw=base64.b64decode(x.get('out-data','')).decode('utf-8-sig').strip()
if raw:
    p=json.loads(raw);(r/'LIVE_PROGRESS.json').write_text(json.dumps(p,indent=2)+'\n')
    print(json.dumps({'phase':p.get('phase'),'completed':len(p.get('runs',[])),'failed':[v['case'] for v in p.get('runs',[]) if v['exit']!=0]}))
else: print('Progress not written yet.')
