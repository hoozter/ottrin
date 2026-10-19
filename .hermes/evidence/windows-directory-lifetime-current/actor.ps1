$ErrorActionPreference='Stop';$result=@{status='running';dispatch_id='69a4498adc36ddb7e2f5ffc7bf12a4ac';binary_sha256='2057a74a29e1f7e644af3ba162a119a58a099400fa105385a3725c3542e319e5';source_sha256='cf98504e05158c29cce68dcef221026ca76065494680cc7e23ca80597e4d43bd'};$process=$null;$temp='C:\OttrinCopyCleanupQA\temp'
try {
 $identity=[Security.Principal.WindowsIdentity]::GetCurrent()
 if($env:USERNAME-ne'OttrinCopyQA'-or([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Wrong or elevated actor'}
 $result.ordinary_user=$true
 if((Get-FileHash C:\OttrinCopyCleanupQA\preparation-probe.exe).Hash.ToLowerInvariant()-ne'2057a74a29e1f7e644af3ba162a119a58a099400fa105385a3725c3542e319e5'){throw 'Probe hash mismatch'}
 if(Test-Path $temp){throw 'Owned namespace exists'};New-Item -ItemType Directory $temp|Out-Null
 $acl=[Security.AccessControl.DirectorySecurity]::new();$acl.SetSecurityDescriptorSddlForm('D:P(A;OICI;FA;;;OW)',[Security.AccessControl.AccessControlSections]::Access);Set-Acl -LiteralPath $temp -AclObject $acl
 $process=Start-Process C:\OttrinCopyCleanupQA\preparation-probe.exe -ArgumentList $temp -WorkingDirectory C:\Windows\System32 -PassThru -RedirectStandardOutput C:\OttrinCopyCleanupQA\results\disposition.stdout -RedirectStandardError C:\OttrinCopyCleanupQA\results\disposition.stderr
 $null=$process.Handle;if(-not$process.WaitForExit(60000)){throw 'Probe remains live'}
 $result.exit=$process.ExitCode;if($result.exit-ne0){throw 'Probe failed'}
 $result.observations=@(Get-Content C:\OttrinCopyCleanupQA\results\disposition.stdout|ForEach-Object{$_|ConvertFrom-Json})
 if($result.observations.Count-ne8){throw 'Incomplete observations'}
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
