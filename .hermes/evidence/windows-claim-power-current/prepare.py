from pathlib import Path
import os
import base64,gzip,hashlib,json,secrets,shutil,subprocess,sys
sys.path.insert(0,str(Path('.hermes/evidence/windows-copy-cleanup-current').resolve()))
import transport as n
b=Path(__file__).resolve().parent
p=Path(os.environ.get("OTTRIN_NATIVE_EVIDENCE_DIR",str(b))).resolve();p.mkdir(parents=True,exist_ok=True)
assert not (p/'DISPATCH.json').exists(),'Resume the existing experiment, do not overwrite it'
rows=[json.loads(l) for l in Path('.hermes/evidence/windows-claim-retirement-interruptions-current/build.jsonl').read_text().splitlines()];assert rows[-1].get('success')
exe=Path(next(x['executable'] for x in rows if x.get('reason')=='compiler-artifact' and x.get('executable') and x.get('profile',{}).get('test') and x.get('target',{}).get('name')=='ottrin_platform'))
with exe.open('rb') as f:digest=hashlib.file_digest(f,'sha256').hexdigest()
prior=json.loads(Path('.hermes/evidence/windows-claim-retirement-interruptions-current/TEST_BINARY.json').read_text());assert digest==prior['compiler_sha256']
phase=os.environ.get('OTTRIN_POWER_PHASE','Prepared');assert phase in ['Prepared','Persisted']
token=secrets.token_hex(16);root='C:\\OttrinCopyCleanupQA\\power-'+token
x={'id':token,'status':'preparing','case':phase+' guest abrupt power stop','root':root,'binary_sha256':digest,'compiler_path':str(exe),'binary_source_commit':json.loads((b/'ACCEPTANCE.json').read_text())['source_commit'],'source_commit':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'phase':phase,'expected_fixture':'copy_transaction::windows::journal::tests::claim_publication::windows_native_copy_temporary_claim_hard_kill_removes_only_its_preparation'}
if phase=='Persisted':x['expected_fixture']='copy_transaction::windows::journal::tests::claim_publication::windows_native_copy_prepared_claim_publishers_race_without_overwrite'
(p/'DISPATCH.json').write_text(json.dumps(x,indent=2)+'\n')
pid=n.start(r"$t=Get-ScheduledTask 'Ottrin Copy Cleanup QA';if($t.State.ToString()-in@('Running','Queued')-or($null-ne$t.Triggers-and@($t.Triggers).Count-ne0)-or[int]$t.Principal.RunLevel-ne0){throw 'Unexpected owned task state/trigger/privilege'};if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like'C:\OttrinCopyCleanupQA\*'}).Count){throw 'Owned executable live'};")
(p/'preflight.pid').write_text(str(pid));n.wait(pid,45)
pid=n.start(r"if(Test-Path C:\OttrinCopyCleanupQA\power-platform.exe){if((Get-FileHash C:\OttrinCopyCleanupQA\power-platform.exe).Hash.ToLowerInvariant()-ne'HASH'){throw 'Existing power binary mismatch'};'reuse'}else{'upload'}".replace('HASH',digest))
(p/'binary-check.pid').write_text(str(pid));checked=n.wait(pid,45)
decision=base64.b64decode(checked['out-data']).decode().strip();assert decision in ['reuse','upload']
if decision=='upload':
 staging=Path('.hermes/packaging-tmp/windows-claim-power');staging.mkdir(exist_ok=False)
 archive=staging/'platform.gz'
 with exe.open('rb') as src,gzip.open(archive,'wb',compresslevel=1) as dest:shutil.copyfileobj(src,dest)
 n.upload(archive,'power-platform.gz')
 pid=n.start(r"$s=[IO.File]::OpenRead('C:\OttrinCopyCleanupQA\power-platform.gz');$z=[IO.Compression.GZipStream]::new($s,[IO.Compression.CompressionMode]::Decompress);$d=[IO.File]::Create('C:\OttrinCopyCleanupQA\power-platform.exe');try{$z.CopyTo($d);$d.Flush($true)}finally{$d.Dispose();$z.Dispose();$s.Dispose()};Remove-Item C:\OttrinCopyCleanupQA\power-platform.gz;if((Get-FileHash C:\OttrinCopyCleanupQA\power-platform.exe).Hash.ToLowerInvariant()-ne'HASH'){throw 'Power binary mismatch'}".replace('HASH',digest))
 (p/'transfer.pid').write_text(str(pid));n.wait(pid,45)
 archive.unlink();staging.rmdir()
actor=p/'actor.ps1';actor.write_text((b/'prepare.ps1').read_text().replace('PHASE',phase).replace('TOKEN',token).replace('BINARY',digest).replace("'ROOT'","'"+root+"'"))
x['actor_sha256']=hashlib.sha256(actor.read_bytes()).hexdigest();x['sync_sha256']=hashlib.sha256((b/'sync.ps1').read_bytes()).hexdigest();(p/'DISPATCH.json').write_text(json.dumps(x,indent=2)+'\n')
n.upload(b/'sync.ps1','power-sync.ps1');n.upload(actor,'actor.ps1')
pid=n.start(r"if((Get-FileHash C:\OttrinCopyCleanupQA\actor.ps1).Hash.ToLowerInvariant()-ne'HASH'){throw 'Actor mismatch'};Start-ScheduledTask 'Ottrin Copy Cleanup QA'".replace('HASH',x['actor_sha256']))
(p/'dispatch.pid').write_text(str(pid));n.wait(pid,45)
x['status']='dispatched';(p/'DISPATCH.json').write_text(json.dumps(x,indent=2)+'\n');print('Power fixture dispatched',flush=True)

subprocess.run([sys.executable,str(b/"cut.py")],check=True)
