$ErrorActionPreference='Stop';$result=@{status='running';dispatch_id='0a1c31bb7b2ed5740b11db0b68d394e5';binary_sha256='ff37690d714bd77b271e0ea448cd28602b3822d5c90cd2481607b7698c6df90e';source_sha256='66e4e76c9b654b05c5d9bb495f198261823fdebe2965bfe876db0ed8aa15473b'};$process=$null;$temp='C:\OttrinCopyCleanupQA\temp'
try {
 $identity=[Security.Principal.WindowsIdentity]::GetCurrent()
 if($env:USERNAME-ne'OttrinCopyQA'-or([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Wrong or elevated actor'}
 $result.ordinary_user=$true
 if((Get-FileHash C:\OttrinCopyCleanupQA\preparation-probe.exe).Hash.ToLowerInvariant()-ne'ff37690d714bd77b271e0ea448cd28602b3822d5c90cd2481607b7698c6df90e'){throw 'Probe hash mismatch'}
 if(Test-Path $temp){throw 'Owned namespace exists'};New-Item -ItemType Directory $temp|Out-Null
 $acl=[Security.AccessControl.DirectorySecurity]::new();$acl.SetSecurityDescriptorSddlForm('D:P(A;OICI;FA;;;OW)',[Security.AccessControl.AccessControlSections]::Access);Set-Acl -LiteralPath $temp -AclObject $acl
 $process=Start-Process C:\OttrinCopyCleanupQA\preparation-probe.exe -ArgumentList $temp -WorkingDirectory C:\Windows\System32 -PassThru -RedirectStandardOutput C:\OttrinCopyCleanupQA\results\disposition.stdout -RedirectStandardError C:\OttrinCopyCleanupQA\results\disposition.stderr
 $null=$process.Handle;if(-not$process.WaitForExit(60000)){throw 'Probe remains live'}
 $result.exit=$process.ExitCode;if($result.exit-ne0){throw 'Probe failed'}
 $result.observations=@(Get-Content C:\OttrinCopyCleanupQA\results\disposition.stdout|ForEach-Object{$_|ConvertFrom-Json})
 if($result.observations.Count-ne1){throw 'Incomplete observations'}
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
