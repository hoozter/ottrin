from pathlib import Path
import os
import hashlib,json,sys
sys.path.insert(0,str(Path('.hermes/evidence/windows-copy-cleanup-current').resolve()))
import transport as n
b=Path(__file__).resolve().parent
p=Path(os.environ.get("OTTRIN_NATIVE_EVIDENCE_DIR",str(b))).resolve();p.mkdir(parents=True,exist_ok=True);x=json.loads((p/'DISPATCH.json').read_text());assert x['status']=='cut'
pid=n.start(r"$t=Get-ScheduledTask 'Ottrin Copy Cleanup QA';if($t.State.ToString()-in@('Running','Queued')){throw 'Old actor still running after boot'};if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like'C:\OttrinCopyCleanupQA\*'}).Count){throw 'Unexpected owned process after cut'};if((Get-FileHash C:\OttrinCopyCleanupQA\power-platform.exe).Hash.ToLowerInvariant()-ne'BINARY'-or(Get-FileHash C:\OttrinCopyCleanupQA\power-sync.ps1).Hash.ToLowerInvariant()-ne'SYNC'){throw 'Power fixtures changed after cut'}".replace('BINARY',x['binary_sha256']).replace('SYNC',x['sync_sha256']))
(p/'postboot.pid').write_text(str(pid));n.wait(pid,45)
a=p/'inspect-actor.ps1';a.write_text((b/'inspect.ps1').read_text().replace("'ROOT'","'"+x['root']+"'").replace('PHASE',x['phase']).replace('TOKEN',x['id']).replace('BINARY',x['binary_sha256']))
n.upload(a,'actor.ps1');pid=n.start("Start-ScheduledTask 'Ottrin Copy Cleanup QA'");n.wait(pid,45)
pid=n.start(r"$deadline=(Get-Date).AddSeconds(60);while($true){if((Get-Date)-gt$deadline){throw 'Inspect deadline; inspect same task'};if(Test-Path C:\OttrinCopyCleanupQA\results\POWER_AFTER.json){$r=Get-Content C:\OttrinCopyCleanupQA\results\POWER_AFTER.json -Raw|ConvertFrom-Json;if($r.dispatch_id-eq'TOKEN'-and(Get-ScheduledTask 'Ottrin Copy Cleanup QA').State.ToString()-notin@('Running','Queued')){break}};Start-Sleep -Milliseconds 100};if($r.status-ne'observed-after-cut'){throw $r.error}".replace('PHASE',x['phase']).replace('TOKEN',x['id']))
(p/'inspect-observer.pid').write_text(str(pid));n.wait(pid,45)
n.download(r'results\POWER_AFTER.json',p/'AFTER.json');r=json.loads((p/'AFTER.json').read_text(encoding='utf-8-sig'));assert r['dispatch_id']==x['id'] and r['ordinary_user']
print(json.dumps(r,indent=2))

if x['phase']=='Persisted':
 assert r['canonical_exists'] and not r['preparation_exists']
 assert set(r['claim_root_names'])=={'short-names.ottrin-claims','short-names.ottrin-claims.root','.preparing-'+('f'*32)}
 before=json.loads((p/'BEFORE.json').read_text(encoding='utf-8-sig'));assert before['canonical_sha256']==r['canonical_sha256']
 n.download(r'results\POWER_OLD_CANONICAL.bin',p/'old-canonical.bin');assert hashlib.sha256((p/'old-canonical.bin').read_bytes()).hexdigest()==r['canonical_sha256']

if x['phase']=='Prepared':
 assert r['preparation_exists']
 n.download(r'results\POWER_PREPARATION.bin',p/'preparation.bin');assert hashlib.sha256((p/'preparation.bin').read_bytes()).hexdigest()==r['preparation_sha256']
