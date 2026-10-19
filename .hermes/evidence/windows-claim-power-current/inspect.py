from pathlib import Path
import hashlib,json,sys
sys.path.insert(0,str(Path('.hermes/evidence/windows-copy-cleanup-current').resolve()))
import transport as n
p=Path(__file__).resolve().parent;x=json.loads((p/'DISPATCH.json').read_text());assert x['status']=='cut'
pid=n.start(r"$t=Get-ScheduledTask 'Ottrin Copy Cleanup QA';if($t.State.ToString()-in@('Running','Queued')){throw 'Old actor still running after boot'};if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like'C:\OttrinCopyCleanupQA\*'}).Count){throw 'Unexpected owned process after cut'};if((Get-FileHash C:\OttrinCopyCleanupQA\power-platform.exe).Hash.ToLowerInvariant()-ne'BINARY'-or(Get-FileHash C:\OttrinCopyCleanupQA\power-sync.ps1).Hash.ToLowerInvariant()-ne'SYNC'){throw 'Power fixtures changed after cut'}".replace('BINARY',x['binary_sha256']).replace('SYNC',x['sync_sha256']))
(p/'postboot.pid').write_text(str(pid));n.wait(pid,45)
a=p/'inspect-actor.ps1';a.write_text((p/'inspect.ps1').read_text().replace("'ROOT'","'"+x['root']+"'").replace('TOKEN',x['id']).replace('BINARY',x['binary_sha256']))
n.upload(a,'actor.ps1');pid=n.start("Start-ScheduledTask 'Ottrin Copy Cleanup QA'");n.wait(pid,45)
pid=n.start(r"$deadline=(Get-Date).AddSeconds(60);while($true){if((Get-Date)-gt$deadline){throw 'Inspect deadline; inspect same task'};if(Test-Path C:\OttrinCopyCleanupQA\results\POWER_AFTER.json){$r=Get-Content C:\OttrinCopyCleanupQA\results\POWER_AFTER.json -Raw|ConvertFrom-Json;if($r.dispatch_id-eq'TOKEN'-and(Get-ScheduledTask 'Ottrin Copy Cleanup QA').State.ToString()-notin@('Running','Queued')){break}};Start-Sleep -Milliseconds 100};if($r.status-ne'observed-after-cut'){throw $r.error}".replace('TOKEN',x['id']))
(p/'inspect-observer.pid').write_text(str(pid));n.wait(pid,45)
n.download(r'results\POWER_AFTER.json',p/'AFTER.json');r=json.loads((p/'AFTER.json').read_text(encoding='utf-8-sig'));assert r['dispatch_id']==x['id'] and r['ordinary_user']
print(json.dumps(r,indent=2))
