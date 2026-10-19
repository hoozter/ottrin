from pathlib import Path
import hashlib,json,sys
sys.path.insert(0,str(Path('.hermes/evidence/windows-copy-cleanup-current').resolve()))
import transport as n
p=Path(__file__).resolve().parent;x=json.loads((p/'DISPATCH.json').read_text());assert x['status']=='dispatched'
if len(sys.argv)>1:
 o=json.loads((p/'OBSERVER.json').read_text());assert o['dispatch_id']==x['id'];pid=o['guest_pid']
else:
 pid=n.start(r"$deadline=(Get-Date).AddSeconds(90);while($true){if((Get-Date)-gt$deadline){throw 'Observer deadline; inspect the same actor before retry'};$r=$null;if(Test-Path C:\OttrinCopyCleanupQA\results\DISPOSITION.json){$v=Get-Content C:\OttrinCopyCleanupQA\results\DISPOSITION.json -Raw|ConvertFrom-Json;if($v.dispatch_id-eq'TOKEN'){$r=$v}};if($r-and(Get-ScheduledTask 'Ottrin Copy Cleanup QA').State.ToString()-notin@('Running','Queued')){break};Start-Sleep -Milliseconds 200};if((Get-ScheduledTaskInfo 'Ottrin Copy Cleanup QA').LastTaskResult-ne0){throw 'Actor failed'}".replace('TOKEN',x['id']))
 o={'dispatch_id':x['id'],'guest_pid':pid,'status':'waiting'};(p/'OBSERVER.json').write_text(json.dumps(o,indent=2)+'\n')
try:n.wait(pid,45)
except TimeoutError:raise SystemExit(75)
n.download(r'results\DISPOSITION.json',p/'RESULTS.json')
r=json.loads((p/'RESULTS.json').read_text(encoding='utf-8-sig'));assert r['dispatch_id']==x['id'] and r['binary_sha256']==x['binary_sha256'] and r['source_sha256']==x['source_sha256'];assert r['status']=='observed' and r['ordinary_user'] and r['owned_temp_absent']
for name in ['stdout','stderr']:
 f=p/('probe.'+name);n.download('results\\disposition.'+name,f);assert hashlib.sha256(f.read_bytes()).hexdigest()==r[name+'_sha256']
assert len(r['observations'])==5
print(json.dumps(r['observations'],indent=2))
o['status']='complete';(p/'OBSERVER.json').write_text(json.dumps(o,indent=2)+'\n')
