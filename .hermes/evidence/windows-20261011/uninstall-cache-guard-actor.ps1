$ProgressPreference='SilentlyContinue';$ErrorActionPreference='Stop'
$b='C:\OttrinQA-20261011';$r="$b\msi-rollback-live-tests"
$result=@{status='running';phase='install'}
function Assert($condition,$message){if(-not $condition){throw $message}}
function Invoke-Msi([string]$Arguments){
 $p=Start-Process 'msiexec.exe' -ArgumentList $Arguments -PassThru
 $null=$p.Handle
 Assert ($p.WaitForExit(120000)) "Owned msiexec still running: $($p.Id)"
 Assert ($p.ExitCode -eq 0) "msiexec exit $($p.ExitCode): $Arguments"
 return $p.ExitCode
}
function Invoke-App([string]$Arguments){
 $start=New-Object Diagnostics.ProcessStartInfo($script:app,$Arguments)
 $start.UseShellExecute=$false;$start.RedirectStandardOutput=$true;$start.RedirectStandardError=$true
 $p=[Diagnostics.Process]::Start($start)
 try {
  $stdout=$p.StandardOutput.ReadToEndAsync();$stderr=$p.StandardError.ReadToEndAsync()
  Assert ($p.WaitForExit(30000)) "CLI still running: $($p.Id)"
  $out=$stdout.Result;$err=$stderr.Result
  Assert ($p.ExitCode -eq 0) "CLI exit $($p.ExitCode): $Arguments : $err"
  return $out
 }finally{if(-not $p.HasExited){$p.Kill();$p.WaitForExit()};$p.Dispose()}
}
function Shared-FileHash($path){
 $stream=[IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
 $sha=[Security.Cryptography.SHA256]::Create()
 try{return [BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-','')}finally{$sha.Dispose();$stream.Dispose()}
}
$lock=$null
try {
 Assert ($env:USERNAME -eq 'OttrinIndexQA') 'Wrong account'
 $id=[Security.Principal.WindowsIdentity]::GetCurrent()
 Assert (-not (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Elevated actor'
 $baseline=Get-Content "$r\result.json" -Raw|ConvertFrom-Json
 $scheduler=New-Object -ComObject Schedule.Service;$scheduler.Connect();$folder=$scheduler.GetFolder('\')
 $task=$folder.GetTask($baseline.task_name)
 Assert ($task.GetInstances(0).Count -eq 1) 'Scheduled daemon not running'
 $db=Join-Path $env:LOCALAPPDATA 'ottrin\search.db'
 Assert (Test-Path $db) 'Search database absent'
 $dbHash=Shared-FileHash $db
 $before=$task.Xml
 $file=Join-Path $env:LOCALAPPDATA 'ottrin\preview_cache\rollback-owned-locked'
 $lock=[IO.File]::Open($file,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
 $app="$b\rollback-current-cli.exe"
 $start=New-Object Diagnostics.ProcessStartInfo($app,'--uninstall');$start.UseShellExecute=$false;$start.RedirectStandardOutput=$true;$start.RedirectStandardError=$true
 $p=[Diagnostics.Process]::Start($start)
 $stdout=$p.StandardOutput.ReadToEndAsync();$stderr=$p.StandardError.ReadToEndAsync()
 Assert ($p.WaitForExit(30000)) 'Owned cleanup CLI timeout'
 $result.exit=$p.ExitCode;$result.stdout=$stdout.Result;$result.stderr=$stderr.Result
 Assert ($p.ExitCode -eq 1) 'Expected blocked-cache error'
 Assert ($result.stderr -match 'Failed to remove.*rollback-owned-locked') 'Missing actual cache failure'
 $after=$folder.GetTask($baseline.task_name)
 Assert ($after.Xml -eq $before -and $after.GetInstances(0).Count -eq 1) 'Blocked cache changed service'
 Assert ((Shared-FileHash $db) -eq $dbHash) 'Blocked cache changed index bytes'
 Assert ((Get-FileHash (Join-Path $env:APPDATA 'ottrin\config.json')).Hash -eq $baseline.preference_hash_before) 'Preferences changed'
 Assert ((Get-FileHash (Join-Path $env:USERPROFILE 'OttrinFixtures\scheduled-daemon-fixture.txt')).Hash -eq $baseline.source_hash_before) 'Source changed'
 $result.database_sha256=$dbHash;$result.task_retained_running=$true;$result.index_bytes_preserved=$true
 $result.source_executable_sha256=(Get-FileHash $app).Hash.ToLower();$result.status='pass';$result.phase='cache-failure-preserved-index'
}catch{$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally{if($lock){$lock.Dispose()}}
$result|ConvertTo-Json -Depth 30|Set-Content "$r\guard.json" -Encoding UTF8
if($result.status -ne 'pass'){exit 1};exit 0
