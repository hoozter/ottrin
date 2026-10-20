from pathlib import Path
import os
import base64,hashlib,json,sys
sys.path.insert(0,str(Path('.hermes/evidence/windows-copy-cleanup-current').resolve()))
import transport as n
b=Path(__file__).resolve().parent
p=Path(os.environ.get("OTTRIN_NATIVE_EVIDENCE_DIR",str(b))).resolve();p.mkdir(parents=True,exist_ok=True);x=json.loads((p/'DISPATCH.json').read_text());r=json.loads((p/'RECOVERY.json').read_text(encoding='utf-8-sig'));assert r['status']=='recovered-name'
if x['status']!='inspected-recovered-cleaned':
 a=p/'cleanup-actor.ps1';a.write_text((b/'cleanup.ps1').read_text().replace("'ROOT'","'"+x['root']+"'").replace('PHASE',x['phase']).replace('TOKEN',x['id']))
 n.upload(a,'actor.ps1');pid=n.start("if((Get-ScheduledTask 'Ottrin Copy Cleanup QA').State.ToString()-ne'Ready'){throw 'Task remains live'};Start-ScheduledTask 'Ottrin Copy Cleanup QA'");n.wait(pid,45)
 pid=n.start(r"$deadline=(Get-Date).AddSeconds(60);while($true){if((Get-Date)-gt$deadline){throw 'Cleanup observer deadline'};if(Test-Path C:\OttrinCopyCleanupQA\results\POWER_CLEANUP.json){$r=Get-Content C:\OttrinCopyCleanupQA\results\POWER_CLEANUP.json -Raw|ConvertFrom-Json;if($r.dispatch_id-eq'TOKEN'-and(Get-ScheduledTask 'Ottrin Copy Cleanup QA').State.ToString()-notin@('Running','Queued')){break}};Start-Sleep -Milliseconds 100};if($r.status-ne'pass'){throw $r.error}".replace('PHASE',x['phase']).replace('TOKEN',x['id']))
 (p/'cleanup-observer.pid').write_text(str(pid));n.wait(pid,45)
 n.download(r'results\POWER_CLEANUP.json',p/'CLEANUP.json');c=json.loads((p/'CLEANUP.json').read_text(encoding='utf-8-sig'));assert c['dispatch_id']==x['id'] and c['status']=='pass' and c['root_absent']
 exports=[('POWER_CANONICAL.bin','canonical.bin',r['canonical_sha256'])]
 for remote,local,expected in exports:
  n.download('results\\'+remote,p/local);assert hashlib.sha256((p/local).read_bytes()).hexdigest()==expected
 pid=n.start(r"if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like'C:\OttrinCopyCleanupQA\*'}).Count){throw 'Owned process remains'};if((Get-FileHash C:\OttrinCopyCleanupQA\power-platform.exe).Hash.ToLowerInvariant()-ne'BINARY'-or(Get-FileHash C:\OttrinCopyCleanupQA\power-sync.ps1).Hash.ToLowerInvariant()-ne'SYNC'){throw 'Fixture cleanup identity mismatch'};Remove-Item C:\OttrinCopyCleanupQA\power-platform.exe,C:\OttrinCopyCleanupQA\power-sync.ps1 -Force".replace('BINARY',x['binary_sha256']).replace('SYNC',x['sync_sha256']))
 (p/'binary-cleanup.pid').write_text(str(pid));n.wait(pid,45)
 x['status']='inspected-recovered-cleaned';(p/'DISPATCH.json').write_text(json.dumps(x,indent=2)+'\n')
 print('Power fixture and binaries cleaned',flush=True)

# Finish export cleanup independently of the already completed root/binary step.
# Pre-cut telemetry can become NUL on disk after the intentional guest cut;
# authoritative copies were downloaded to the host before destruction.
export_receipt=p/'EXPORT_CLEANUP.json'
if not export_receipt.exists():
 exports=[('POWER_READY.json','BEFORE.json',True),('POWER_LIVE.json','LIVE.json',True),('POWER_AFTER.json','AFTER.json',False),('POWER_RECOVERY.json','RECOVERY.json',False),('POWER_CLEANUP.json','CLEANUP.json',False),('POWER_CANONICAL.bin','canonical.bin',False),('power-recover.stdout','recover.stdout',False),('power-recover.stderr','recover.stderr',False)]
 if x['phase']=='Prepared':exports.append(('POWER_PREPARATION.bin','preparation.bin',False))
 else:exports.append(('POWER_OLD_CANONICAL.bin','old-canonical.bin',False))
 items=[]
 for remote,local,volatile in exports:
  data=(p/local).read_bytes();items.append({'name':remote,'sha256':hashlib.sha256(data).hexdigest(),'size':len(data),'volatile':volatile})
 actor_hash=hashlib.sha256((p/'cleanup-actor.ps1').read_bytes()).hexdigest()
 script=r"$t=Get-ScheduledTask 'Ottrin Copy Cleanup QA';if($t.State.ToString()-ne'Ready'){throw 'Export owner still running'};if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like'C:\OttrinCopyCleanupQA\*'}).Count){throw 'Owned process remains'};if((Get-FileHash C:\OttrinCopyCleanupQA\actor.ps1).Hash.ToLowerInvariant()-ne'ACTOR'){throw 'Another export owner replaced the actor'};$r=Get-Content C:\OttrinCopyCleanupQA\results\POWER_CLEANUP.json -Raw|ConvertFrom-Json;if($r.dispatch_id-ne'TOKEN'-or$r.status-ne'pass'-or-not$r.root_absent){throw 'Wrong completed export owner'};$items='ITEMS'|ConvertFrom-Json;$removed=@();foreach($i in $items){$f=Join-Path 'C:\OttrinCopyCleanupQA\results' $i.name;$bytes=[IO.File]::ReadAllBytes($f);$hash=(Get-FileHash -LiteralPath $f).Hash.ToLowerInvariant();$lost=$false;if($hash-ne$i.sha256){if(-not$i.volatile-or$bytes.Length-ne$i.size-or@($bytes|Where-Object{$_-ne0}).Count-ne0){throw ('Export identity mismatch: '+$i.name)};$lost=$true};$removed+=@{name=$i.name;sha256=$hash;volatile_bytes_lost=$lost}};foreach($i in $items){$f=Join-Path 'C:\OttrinCopyCleanupQA\results' $i.name;Remove-Item -LiteralPath $f -Force;if(Test-Path -LiteralPath $f){throw 'Owned export remains'}};@{status='pass';dispatch_id='TOKEN';removed=$removed}|ConvertTo-Json -Depth 4 -Compress"
 script=script.replace('ACTOR',actor_hash).replace('TOKEN',x['id']).replace('ITEMS',json.dumps(items))
 pid=n.start(script);(p/'export-cleanup.pid').write_text(str(pid));completed=n.wait(pid,45)
 report=json.loads(base64.b64decode(completed['out-data']).decode('utf-8-sig'));assert report['status']=='pass' and report['dispatch_id']==x['id'] and len(report['removed'])==len(items)
 export_receipt.write_text(json.dumps(report,indent=2)+'\n')
else:
 report=json.loads(export_receipt.read_text());assert report['status']=='pass' and report['dispatch_id']==x['id']
print('Owned power exports cleaned',flush=True)
