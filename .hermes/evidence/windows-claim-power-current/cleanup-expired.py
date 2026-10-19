from pathlib import Path
import json,sys
sys.path.insert(0,str(Path('.hermes/evidence/windows-copy-cleanup-current').resolve()))
import transport as n
p=Path(__file__).resolve().parent;x=json.loads((p/'DISPATCH.json').read_text());d=p/'expired-before-cut';d.mkdir(exist_ok=False)
for remote,local in [('POWER_READY.json','BEFORE.json'),('POWER_EARLY_EXIT.json','EARLY_EXIT.json')]:n.download('results\\'+remote,d/local)
actor=p/'cleanup-expired.ps1';actor.write_text(r"$ErrorActionPreference='Stop';if($env:USERNAME-ne'OttrinCopyQA'){throw 'Wrong cleanup actor'};$root='ROOT';if((Get-Content -LiteralPath (Join-Path $root 'guard') -Raw)-ne'TOKEN'){throw 'Fixture ownership mismatch'};if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like'C:\OttrinCopyCleanupQA\*'}).Count){throw 'Owned executable still live'};Remove-Item -LiteralPath $root -Recurse -Force;@{dispatch_id='TOKEN';owned_root_absent=(-not(Test-Path $root))}|ConvertTo-Json|Set-Content C:\OttrinCopyCleanupQA\results\POWER_EXPIRED_CLEANUP.json".replace("'ROOT'","'"+x['root']+"'").replace('TOKEN',x['id']))
n.upload(actor,'actor.ps1');pid=n.start("if((Get-ScheduledTask 'Ottrin Copy Cleanup QA').State.ToString()-ne'Ready'){throw 'Task not terminal'};Start-ScheduledTask 'Ottrin Copy Cleanup QA'");n.wait(pid,45)
pid=n.start(r"while((Get-ScheduledTask 'Ottrin Copy Cleanup QA').State.ToString()-in@('Running','Queued')){Start-Sleep -Milliseconds 100};if((Get-ScheduledTaskInfo 'Ottrin Copy Cleanup QA').LastTaskResult-ne0){throw 'Cleanup failed'}")
(p/'expired-cleanup.pid').write_text(str(pid));n.wait(pid,45);n.download(r'results\POWER_EXPIRED_CLEANUP.json',d/'CLEANUP.json')
r=json.loads((d/'CLEANUP.json').read_text(encoding='utf-8-sig'));assert r['dispatch_id']==x['id'] and r['owned_root_absent']
for name in ['DISPATCH.json','actor.ps1']:(p/name).rename(d/name)
(d/'REJECTED_CUT.json').write_text(json.dumps({'reason':'Barrier expired during controller preparation; terminal actor and absent child verified; no virsh destroy executed','cleanup':'Owned fixture removed by ordinary account after guard-token validation'},indent=2)+'\n')
print('Expired uncut fixture cleaned; replay is safe',flush=True)
