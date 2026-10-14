$ProgressPreference='SilentlyContinue';$ErrorActionPreference='Stop'
$b='C:\OttrinQA-20261011'
$r="$b\index-upgrade-tests"
$result=@{status='running';phase='seed'}
function Assert($condition,$message){if(-not $condition){throw $message}}
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
function Check-Payload($manifest){
 $hashes=Get-Content "$b\$manifest" -Raw | ConvertFrom-Json
 foreach($p in $hashes.PSObject.Properties){
  $actual=(Get-FileHash (Join-Path $script:install $p.Name)).Hash.ToLower()
  Assert ($actual -eq $p.Value) "Payload differs: $($p.Name)"
 }
 return @($hashes.PSObject.Properties).Count
}
try {
 Assert ($env:USERNAME -eq 'OttrinIndexQA') 'Wrong account'
 $id=[Security.Principal.WindowsIdentity]::GetCurrent()
 Assert (-not (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Elevated actor'
 $install=Join-Path $env:LOCALAPPDATA 'Programs\Ottrin'
 Assert (-not (Test-Path $install)) 'Unexpected existing installation'
 $pref=Join-Path $env:APPDATA 'ottrin\config.json'
 $config=Get-Content $pref -Raw | ConvertFrom-Json
 $config.search.refresh_interval_hours=6
 [IO.File]::WriteAllText($pref,($config | ConvertTo-Json -Depth 30))
 $hash=(Get-FileHash $pref).Hash
 $source=Join-Path $env:USERPROFILE 'OttrinFixtures\scheduled-daemon-fixture.txt'
 $sourceHash=(Get-FileHash $source).Hash
 $result.preference_hash_before=$hash;$result.source_hash_before=$sourceHash
 $taskName='Ottrin Index '+$id.User.Value
 $scheduler=New-Object -ComObject Schedule.Service;$scheduler.Connect();$folder=$scheduler.GetFolder('\')
 $result.phase='old-install'
 $p=Start-Process "$b\index-upgrade-old.exe" -ArgumentList '/S' -PassThru -Wait
 Assert ($p.ExitCode -eq 0) "Old install exit $($p.ExitCode)"
 $app=Join-Path $install 'bin\ottrin.exe'
 $result.old_payload_files=Check-Payload 'index-upgrade-old-hashes.json'
 $old=$folder.GetTask($taskName)
 Assert ($old.Definition.Actions.Item(1).Arguments -eq '--index-update') 'Old task action differs'
 $result.old_task_xml=$old.Xml
 $result.phase='upgrade'
 $p=Start-Process "$b\index-upgrade-new.exe" -ArgumentList '/S' -PassThru -Wait
 Assert ($p.ExitCode -eq 0) "Upgrade exit $($p.ExitCode)"
 $result.upgrade_exit=$p.ExitCode
 $result.new_payload_files=Check-Payload 'index-upgrade-new-hashes.json'
 $new=$folder.GetTask($taskName)
 Assert ($new.Definition.Actions.Item(1).Arguments -eq '--index-daemon') 'Task not migrated to daemon'
 Assert ($new.Definition.Triggers.Count -eq 1 -and $new.Definition.Triggers.Item(1).Type -eq 9) 'Task not migrated to logon'
 Assert ($new.State -eq 4) "Task not running: $($new.State)"
 $owner=$new.Definition.Principal.UserId
 if($owner -notmatch '^S-1-'){ $owner=(New-Object Security.Principal.NTAccount($owner)).Translate([Security.Principal.SecurityIdentifier]).Value }
 Assert ($owner -eq $id.User.Value) 'Task owner differs'
 $result.task_owner_sid=$owner
 $result.new_task_xml=$new.Xml
 $result.update=Invoke-App 'index update --json'
 $q=Invoke-App 'search scheduled-daemon-fixture --json'
 $result.query=$q | ConvertFrom-Json
 Assert ($result.query.total -eq 1) 'Expected one fixture result'
 $result.service_status=Invoke-App 'index service-status'
 Assert ((Get-FileHash $pref).Hash -eq $hash) 'Preferences changed by upgrade'
 Assert ((Get-FileHash $source).Hash -eq $sourceHash) 'User source changed'
 $result.preference_hash=$hash;$result.source_hash=$sourceHash
 $result.phase='upgraded';$result.status='pass'
} catch {$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
$result | ConvertTo-Json -Depth 30 | Set-Content "$r\result.json" -Encoding UTF8
if($result.status -ne 'pass'){exit 1}
exit 0
