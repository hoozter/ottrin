$ErrorActionPreference='Stop';$result=@{status='running';dispatch_id='caa69f7e0f88b23e876acf2acbfc230b';binary_sha256='bda618ad8d983cda92c909fd846ce973f6c7361a5d7e091b234aa6aee7b81157';source_sha256='dc37237f5a105609bca697caba4cf4931a87228d1fad9ee581f73ed2174de5aa'};$process=$null;$temp='C:\OttrinCopyCleanupQA\temp'
try {
 $identity=[Security.Principal.WindowsIdentity]::GetCurrent()
 if($env:USERNAME-ne'OttrinCopyQA'-or([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Wrong or elevated actor'}
 $result.ordinary_user=$true
 if((Get-FileHash C:\OttrinCopyCleanupQA\preparation-probe.exe).Hash.ToLowerInvariant()-ne'bda618ad8d983cda92c909fd846ce973f6c7361a5d7e091b234aa6aee7b81157'){throw 'Probe hash mismatch'}
 if(Test-Path $temp){throw 'Owned namespace exists'};New-Item -ItemType Directory $temp|Out-Null
 $acl=[Security.AccessControl.DirectorySecurity]::new();$acl.SetSecurityDescriptorSddlForm('D:P(A;OICI;FA;;;OW)',[Security.AccessControl.AccessControlSections]::Access);Set-Acl -LiteralPath $temp -AclObject $acl
 $process=Start-Process C:\OttrinCopyCleanupQA\preparation-probe.exe -ArgumentList $temp -WorkingDirectory C:\Windows\System32 -PassThru -RedirectStandardOutput C:\OttrinCopyCleanupQA\results\disposition.stdout -RedirectStandardError C:\OttrinCopyCleanupQA\results\disposition.stderr
 $null=$process.Handle;if(-not$process.WaitForExit(60000)){throw 'Probe remains live'}
 $result.exit=$process.ExitCode;if($result.exit-ne0){throw 'Probe failed'}
 $result.observations=@(Get-Content C:\OttrinCopyCleanupQA\results\disposition.stdout|ForEach-Object{$_|ConvertFrom-Json})
 if($result.observations.Count-ne7){throw 'Incomplete observations'}
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
