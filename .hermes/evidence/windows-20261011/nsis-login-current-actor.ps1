param([ValidateSet('Install','Verify','Remove')][string]$Phase)
$ProgressPreference='SilentlyContinue';$ErrorActionPreference='Stop'
$b='C:\OttrinQA-20261011';$r="$b\nsis-login-tests";$result=@{phase=$Phase;status='running'}
function Assert($ok,$message){if(-not $ok){throw $message}}
function Invoke-App($arguments){
 $start=[Diagnostics.ProcessStartInfo]::new($script:app,$arguments);$start.UseShellExecute=$false;$start.RedirectStandardOutput=$true;$start.RedirectStandardError=$true
 $p=[Diagnostics.Process]::Start($start)
 try{$out=$p.StandardOutput.ReadToEndAsync();$err=$p.StandardError.ReadToEndAsync();Assert ($p.WaitForExit(30000)) 'Owned CLI timeout';Assert ($p.ExitCode -eq 0) "CLI failure: $($err.Result)";return $out.Result}
 finally{if(-not $p.HasExited){$p.Kill();$p.WaitForExit()};$p.Dispose()}
}
try {
 Assert ($env:USERNAME -eq 'OttrinIndexQA') 'Wrong account'
 $id=[Security.Principal.WindowsIdentity]::GetCurrent();Assert (-not ([Security.Principal.WindowsPrincipal]::new($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Elevated actor'
 $install=Join-Path $env:LOCALAPPDATA 'Programs\Ottrin';$app=Join-Path $install 'bin\ottrin.exe'
 $pref=Join-Path $env:APPDATA 'ottrin\config.json';$source=Join-Path $env:USERPROFILE 'OttrinFixtures\scheduled-daemon-fixture.txt'
 $scheduler=New-Object -ComObject Schedule.Service;$scheduler.Connect();$folder=$scheduler.GetFolder('\');$name='Ottrin Index '+$id.User.Value
 if($Phase -eq 'Install'){
  Assert (-not(Test-Path $install)) 'Unexpected existing app'
  $result.preference_hash=(Get-FileHash $pref).Hash;$result.source_hash=(Get-FileHash $source).Hash
  $result.boot=(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToString('o')
  $result.installer_sha256=(Get-FileHash "$b\nsis-login-installer.exe").Hash.ToLower()
  Assert ($result.installer_sha256 -eq '48f6d318a7a1bc702406c84ddea9645aedaaeca2d03c4ad7e61b6f4f577bba2e') 'Wrong installer'
  $p=Start-Process "$b\nsis-login-installer.exe" -ArgumentList '/S' -PassThru;$null=$p.Handle
  Assert ($p.WaitForExit(120000) -and $p.ExitCode -eq 0) 'Install failed'
  $result.query=(Invoke-App 'search scheduled-daemon-fixture --json')|ConvertFrom-Json
  Assert ($result.query.total -eq 1) 'Initial query not one'
  $result.daemon_instances=$folder.GetTask($name).GetInstances(0).Count;Assert ($result.daemon_instances -eq 1) 'Daemon not running'
 }else{
  $prior=Get-Content "$r\Install.json" -Raw|ConvertFrom-Json;Assert ($prior.status -eq 'pass') 'Install prerequisite failed'
  if($Phase -eq 'Verify'){
   $boot=(Get-CimInstance Win32_OperatingSystem).LastBootUpTime
   Assert ($boot.ToString('o') -ne $prior.boot) 'Not a new boot'
   $task=$folder.GetTask($name)
   Assert ($task.GetInstances(0).Count -eq 1 -and $task.State -eq 4) 'Login task not running before CLI'
   Assert ($task.LastRunTime -ge $boot) 'Task did not run this boot'
   Assert ($task.Definition.Triggers.Count -eq 1 -and $task.Definition.Triggers.Item(1).Type -eq 9) 'Wrong trigger'
   $result.boot=$boot.ToString('o');$result.task_last_run=$task.LastRunTime.ToString('o');$result.task_last_result=$task.LastTaskResult;$result.task_xml=$task.Xml
   $result.query=(Invoke-App 'search scheduled-daemon-fixture --json')|ConvertFrom-Json
   Assert ($result.query.total -eq 1) 'Retained query not one'
   $result.index_status=(Invoke-App 'index status --json')|ConvertFrom-Json
   Assert ($result.index_status.watcher_ready) 'Watch not ready'
  }else{
   $p=Start-Process "$install\Uninstall.exe" -ArgumentList '/S' -PassThru;$null=$p.Handle
   Assert ($p.WaitForExit(30000) -and $p.ExitCode -eq 0) 'Uninstall bootstrap failed'
   $deadline=[DateTime]::UtcNow.AddSeconds(30)
   while((Test-Path $install) -and [DateTime]::UtcNow -lt $deadline){Start-Sleep -Milliseconds 100}
   Assert (-not(Test-Path $install)) 'Install directory remains'
   $absent=$false;try{$null=$folder.GetTask($name)}catch{if($_.Exception.GetBaseException().HResult -eq -2147024894){$absent=$true}else{throw}}
   Assert $absent 'Task remains'
   Assert (-not(Test-Path (Join-Path $env:LOCALAPPDATA 'ottrin\search.db'))) 'Index remains'
   $result.bootstrap_exit=$p.ExitCode;$result.task_removed=$true;$result.install_removed=$true
  }
  Assert ((Get-FileHash $pref).Hash -eq $prior.preference_hash) 'Preferences changed'
  Assert ((Get-FileHash $source).Hash -eq $prior.source_hash) 'Source changed'
  $result.preference_hash=(Get-FileHash $pref).Hash;$result.source_hash=(Get-FileHash $source).Hash
 }
 $result.status='pass'
}catch{$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
$result|ConvertTo-Json -Depth 30|Set-Content "$r\$Phase.json" -Encoding UTF8
if($result.status -ne 'pass'){exit 1};exit 0
