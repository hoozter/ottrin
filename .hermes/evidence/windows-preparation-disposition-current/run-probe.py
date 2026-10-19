from pathlib import Path
import hashlib,json,secrets,sys,os
sys.path.insert(0,str(Path('.hermes/evidence/windows-copy-cleanup-current').resolve()))
import transport as n
p=Path(os.environ.get('OTTRIN_NATIVE_EVIDENCE_DIR',str(Path(__file__).resolve().parent))).resolve();p.mkdir(parents=True,exist_ok=True)
assert not (p/'DISPATCH.json').exists(),'Resume existing probe; do not overwrite its dispatch'
count=int(os.environ.get('OTTRIN_PROBE_OBSERVATIONS','5'));assert 0<count<=64
exe=Path(os.environ.get('OTTRIN_PROBE_EXE','.hermes/packaging-tmp/preparation-disposition/probe.exe'))
digest=hashlib.sha256(exe.read_bytes()).hexdigest();token=secrets.token_hex(16)
source=hashlib.sha256((p/'probe.c').read_bytes()).hexdigest()
pid=n.start(r"$t=Get-ScheduledTask 'Ottrin Copy Cleanup QA';if($t.State.ToString()-in@('Running','Queued')){throw 'Owned task still live'};if([int]$t.Principal.RunLevel-ne0){throw 'Elevated actor'};if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like'C:\OttrinCopyCleanupQA\*'}).Count){throw 'Owned executable remains'};if((Test-Path C:\OttrinCopyCleanupQA\temp)-or(Test-Path C:\OttrinCopyCleanupQA\preparation-probe.exe)){throw 'Owned probe namespace already exists'}")
(p/'preflight.pid').write_text(str(pid));n.wait(pid,45)
n.upload(exe,'preparation-probe.exe')
actor=p/'actor.ps1'
actor.write_text(r'''$ErrorActionPreference='Stop';$result=@{status='running';dispatch_id='TOKEN';binary_sha256='HASH';source_sha256='SOURCE'};$process=$null;$temp='C:\OttrinCopyCleanupQA\temp'
try {
 $identity=[Security.Principal.WindowsIdentity]::GetCurrent()
 if($env:USERNAME-ne'OttrinCopyQA'-or([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Wrong or elevated actor'}
 $result.ordinary_user=$true
 if((Get-FileHash C:\OttrinCopyCleanupQA\preparation-probe.exe).Hash.ToLowerInvariant()-ne'HASH'){throw 'Probe hash mismatch'}
 if(Test-Path $temp){throw 'Owned namespace exists'};New-Item -ItemType Directory $temp|Out-Null
 $acl=[Security.AccessControl.DirectorySecurity]::new();$acl.SetSecurityDescriptorSddlForm('D:P(A;OICI;FA;;;OW)',[Security.AccessControl.AccessControlSections]::Access);Set-Acl -LiteralPath $temp -AclObject $acl
 $process=Start-Process C:\OttrinCopyCleanupQA\preparation-probe.exe -ArgumentList $temp -WorkingDirectory C:\Windows\System32 -PassThru -RedirectStandardOutput C:\OttrinCopyCleanupQA\results\disposition.stdout -RedirectStandardError C:\OttrinCopyCleanupQA\results\disposition.stderr
 $null=$process.Handle;if(-not$process.WaitForExit(60000)){throw 'Probe remains live'}
 $result.exit=$process.ExitCode;if($result.exit-ne0){throw 'Probe failed'}
 $result.observations=@(Get-Content C:\OttrinCopyCleanupQA\results\disposition.stdout|ForEach-Object{$_|ConvertFrom-Json})
 if($result.observations.Count-neOBSERVATIONS){throw 'Incomplete observations'}
 $result.status='observed'
} catch {$result.status='fail';$result.error=$_.Exception.Message}
finally {
 try {
  if($process-and-not$process.HasExited){& C:\Windows\System32\taskkill.exe /PID $process.Id /T /F|Out-Null;if(-not$process.WaitForExit(10000)){throw 'Probe did not stop'}}
  if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like'C:\OttrinCopyCleanupQA\*'}).Count){throw 'Owned executable remains'}
  if(Test-Path $temp){Remove-Item -LiteralPath $temp -Recurse -Force}
  $result.owned_temp_absent=-not(Test-Path $temp)
  foreach($name in @('stdout','stderr')){$result[$name+'_sha256']=(Get-FileHash ('C:\OttrinCopyCleanupQA\results\disposition.'+$name)).Hash.ToLowerInvariant()}
 } catch {$result.status='fail';$result.teardown_error=$_.ToString()}
 $result|ConvertTo-Json -Depth 5|Set-Content C:\OttrinCopyCleanupQA\results\DISPOSITION.json.tmp
 Move-Item C:\OttrinCopyCleanupQA\results\DISPOSITION.json.tmp C:\OttrinCopyCleanupQA\results\DISPOSITION.json -Force
}
if($result.status-ne'observed'){exit 1}
'''.replace('OBSERVATIONS',str(count)).replace('TOKEN',token).replace('HASH',digest).replace('SOURCE',source))
actor_hash=hashlib.sha256(actor.read_bytes()).hexdigest()
(p/'DISPATCH.json').write_text(json.dumps({'id':token,'status':'preparing','binary_sha256':digest,'source_sha256':source,'actor_sha256':actor_hash,'expected_observations':count,'scope':'Ordinary-account native lifetime observations; not production acceptance'},indent=2)+'\n')
n.upload(actor,'actor.ps1')
pid=n.start(r"$t=Get-ScheduledTask 'Ottrin Copy Cleanup QA';if($t.State.ToString()-in@('Running','Queued')){throw 'Owned task remains live'};if((Get-FileHash C:\OttrinCopyCleanupQA\actor.ps1).Hash.ToLowerInvariant()-ne'ACTOR'){throw 'Actor hash mismatch'};Start-ScheduledTask $t.TaskName".replace('ACTOR',actor_hash))
(p/'dispatch.pid').write_text(str(pid));n.wait(pid,45)
x=json.loads((p/'DISPATCH.json').read_text());x['status']='dispatched';(p/'DISPATCH.json').write_text(json.dumps(x,indent=2)+'\n')
print('Probe dispatched',flush=True)
