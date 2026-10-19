from pathlib import Path
import os
import json,sys
sys.path.insert(0,str(Path('.hermes/evidence/windows-copy-cleanup-current').resolve()))
import transport as n
b=Path(__file__).resolve().parent
p=Path(os.environ.get("OTTRIN_NATIVE_EVIDENCE_DIR",str(b))).resolve();p.mkdir(parents=True,exist_ok=True);x=json.loads((p/'DISPATCH.json').read_text());before=json.loads((p/'BEFORE.json').read_text(encoding='utf-8-sig'));after=json.loads((p/'AFTER.json').read_text(encoding='utf-8-sig'))
assert x['status']=='cut' and after['status']=='observed-after-cut'
for key in (['source_sha256','public_sha256','foreign_sha256']+(['preparation_sha256'] if x['phase']=='Prepared' else [])):assert before[key]==after[key]
a=p/'recover-actor.ps1';a.write_text((b/'recover.ps1').read_text().replace("'ROOT'","'"+x['root']+"'").replace('PHASE',x['phase']).replace('TOKEN',x['id']).replace('BINARY',x['binary_sha256']))
n.upload(a,'actor.ps1');pid=n.start("if((Get-ScheduledTask 'Ottrin Copy Cleanup QA').State.ToString()-ne'Ready'){throw 'Task not idle'};Start-ScheduledTask 'Ottrin Copy Cleanup QA'");n.wait(pid,45)
pid=n.start(r"$deadline=(Get-Date).AddSeconds(60);while($true){if((Get-Date)-gt$deadline){throw 'Recovery observer deadline; inspect same actor'};if(Test-Path C:\OttrinCopyCleanupQA\results\POWER_RECOVERY.json){$r=Get-Content C:\OttrinCopyCleanupQA\results\POWER_RECOVERY.json -Raw|ConvertFrom-Json;if($r.dispatch_id-eq'TOKEN'-and(Get-ScheduledTask 'Ottrin Copy Cleanup QA').State.ToString()-notin@('Running','Queued')){break}};Start-Sleep -Milliseconds 100};if($r.status-ne'recovered-name'){throw $r.error}".replace('PHASE',x['phase']).replace('TOKEN',x['id']))
(p/'recover-observer.pid').write_text(str(pid));n.wait(pid,45)
n.download(r'results\POWER_RECOVERY.json',p/'RECOVERY.json');r=json.loads((p/'RECOVERY.json').read_text(encoding='utf-8-sig'));assert r['dispatch_id']==x['id'] and r['ordinary_user'] and r['fresh_admission'] and r['owner_exit']==86
for key in ['source_sha256','public_sha256','foreign_sha256']:assert before[key]==r[key]
if x['phase']=='Prepared':assert not r['preparation_exists']
for name in ['stdout','stderr']:n.download('results\\power-recover.'+name,p/('recover.'+name))
print(json.dumps(r,indent=2))

if x['phase']=='Persisted':assert r['canonical_sha256']!=before['canonical_sha256']
