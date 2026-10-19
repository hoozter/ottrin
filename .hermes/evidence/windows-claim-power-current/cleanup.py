from pathlib import Path
import os
import hashlib,json,sys
sys.path.insert(0,str(Path('.hermes/evidence/windows-copy-cleanup-current').resolve()))
import transport as n
b=Path(__file__).resolve().parent
p=Path(os.environ.get("OTTRIN_NATIVE_EVIDENCE_DIR",str(b))).resolve();p.mkdir(parents=True,exist_ok=True);x=json.loads((p/'DISPATCH.json').read_text());r=json.loads((p/'RECOVERY.json').read_text(encoding='utf-8-sig'));assert r['status']=='recovered-name'
a=p/'cleanup-actor.ps1';a.write_text((b/'cleanup.ps1').read_text().replace("'ROOT'","'"+x['root']+"'").replace('PHASE',x['phase']).replace('TOKEN',x['id']))
n.upload(a,'actor.ps1');pid=n.start("if((Get-ScheduledTask 'Ottrin Copy Cleanup QA').State.ToString()-ne'Ready'){throw 'Task remains live'};Start-ScheduledTask 'Ottrin Copy Cleanup QA'");n.wait(pid,45)
pid=n.start(r"$deadline=(Get-Date).AddSeconds(60);while($true){if((Get-Date)-gt$deadline){throw 'Cleanup observer deadline'};if(Test-Path C:\OttrinCopyCleanupQA\results\POWER_CLEANUP.json){$r=Get-Content C:\OttrinCopyCleanupQA\results\POWER_CLEANUP.json -Raw|ConvertFrom-Json;if($r.dispatch_id-eq'TOKEN'-and(Get-ScheduledTask 'Ottrin Copy Cleanup QA').State.ToString()-notin@('Running','Queued')){break}};Start-Sleep -Milliseconds 100};if($r.status-ne'pass'){throw $r.error}".replace('PHASE',x['phase']).replace('TOKEN',x['id']))
(p/'cleanup-observer.pid').write_text(str(pid));n.wait(pid,45)
n.download(r'results\POWER_CLEANUP.json',p/'CLEANUP.json');c=json.loads((p/'CLEANUP.json').read_text(encoding='utf-8-sig'));assert c['dispatch_id']==x['id'] and c['status']=='pass' and c['root_absent']
exports=[('POWER_CANONICAL.bin','canonical.bin',r['canonical_sha256'])]
if x['phase']=='Prepared':exports.append(('POWER_PREPARATION.bin','preparation.bin',r['preparation_sha256']))
for remote,local,expected in exports:
 n.download('results\\'+remote,p/local);assert hashlib.sha256((p/local).read_bytes()).hexdigest()==expected
pid=n.start(r"if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like'C:\OttrinCopyCleanupQA\*'}).Count){throw 'Owned process remains'};if((Get-FileHash C:\OttrinCopyCleanupQA\power-platform.exe).Hash.ToLowerInvariant()-ne'BINARY'-or(Get-FileHash C:\OttrinCopyCleanupQA\power-sync.ps1).Hash.ToLowerInvariant()-ne'SYNC'){throw 'Fixture cleanup identity mismatch'};Remove-Item C:\OttrinCopyCleanupQA\power-platform.exe,C:\OttrinCopyCleanupQA\power-sync.ps1 -Force".replace('BINARY',x['binary_sha256']).replace('SYNC',x['sync_sha256']))
(p/'binary-cleanup.pid').write_text(str(pid));n.wait(pid,45)
x['status']='inspected-recovered-cleaned';(p/'DISPATCH.json').write_text(json.dumps(x,indent=2)+'\n')
print('Power fixture and binaries cleaned',flush=True)
