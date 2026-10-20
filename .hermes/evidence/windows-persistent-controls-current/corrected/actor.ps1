$ErrorActionPreference='Stop';$result=@{status='running';dispatch_id='cbc1639634ad76375cafe9bf5bb497ff';test_arguments='copy_transaction::windows::journal:: --skip copy_transaction::windows::journal::tests::cleanup:: --nocapture --test-threads=1'};$process=$null;$temp='C:\OttrinCopyCleanupQA\temp'
try {
 if($env:USERNAME-ne'OttrinCopyQA'){throw 'Wrong test account'}
 $identity=[Security.Principal.WindowsIdentity]::GetCurrent();if(([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Elevated test account'}
 $result.ordinary_user=$true;$result.test_binary_hash=(Get-FileHash C:\OttrinCopyCleanupQA\platform.exe).Hash.ToLowerInvariant();if($result.test_binary_hash-ne'92e90907ac6cc8027ee4617b5b28d4039c3f7e163a117d3d9282535eeda874c8'){throw 'Actor binary hash mismatch'}
 if(Test-Path $temp){throw 'Owned temporary namespace exists'};New-Item -ItemType Directory $temp|Out-Null;$acl=[Security.AccessControl.DirectorySecurity]::new();$acl.SetSecurityDescriptorSddlForm('D:P(A;OICI;FA;;;OW)',[Security.AccessControl.AccessControlSections]::Access);Set-Acl -LiteralPath $temp -AclObject $acl;$env:TEMP=$temp;$env:TMP=$temp
 $process=Start-Process C:\OttrinCopyCleanupQA\platform.exe -ArgumentList 'copy_transaction::windows::journal:: --skip copy_transaction::windows::journal::tests::cleanup:: --nocapture --test-threads=1' -WorkingDirectory C:\Windows\System32 -PassThru -RedirectStandardOutput C:\OttrinCopyCleanupQA\results\platform.stdout -RedirectStandardError C:\OttrinCopyCleanupQA\results\platform.stderr
 $null=$process.Handle;if(-not$process.WaitForExit(900000)){throw "Owned test remains live: $($process.Id)"}
 $result.exit=$process.ExitCode;if($result.exit-ne0){throw 'Native cleanup observation failed'}
 $result.temporary_entries=@(Get-ChildItem -LiteralPath $temp -Force).Count;if($result.temporary_entries){throw 'Tests left temporary entries'}
 $result.status='pass'
} catch {$result.status='fail';$result.error=$_.Exception.Message}
finally {
 try {
  if($process-and-not$process.HasExited){
   & C:\Windows\System32\taskkill.exe /PID $process.Id /T /F|Out-Null
   if(-not$process.WaitForExit(10000)){throw 'Owned native test did not stop'}
  }
  if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like'C:\OttrinCopyCleanupQA\*'}).Count){throw 'Owned native processes remain during teardown'}
  if(Test-Path $temp){
   for($attempt=0;$attempt-lt5;$attempt++){
    try {Remove-Item -LiteralPath $temp -Recurse -Force;break}
    catch {if($attempt-eq4){throw};Start-Sleep -Milliseconds 200}
   }
  }
 } catch {$result.status='fail';$result.teardown_error=$_.ToString()}
 finally {
  try {if($process){foreach($name in @('stdout','stderr')){$path='C:\OttrinCopyCleanupQA\results\platform.'+$name;if(Test-Path $path){$result[$name+'_sha256']=(Get-FileHash $path).Hash.ToLowerInvariant()}}}} catch {$result.status='fail';$result.artifact_error=$_.Exception.Message}
  $result|ConvertTo-Json|Set-Content C:\OttrinCopyCleanupQA\results\RESULTS.json.tmp
  Move-Item -LiteralPath C:\OttrinCopyCleanupQA\results\RESULTS.json.tmp -Destination C:\OttrinCopyCleanupQA\results\RESULTS.json -Force
 }
}
if($result.status-ne'pass'){exit 1}
