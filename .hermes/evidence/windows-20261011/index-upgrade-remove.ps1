$ProgressPreference='SilentlyContinue';$ErrorActionPreference='Stop'
$b='C:\OttrinQA-20261011';$r="$b\index-upgrade-tests"
$prior=Get-Content "$r\verified.json" -Raw|ConvertFrom-Json
$result=@{status='running';phase='remove'}
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
try {
 Assert ($env:USERNAME -eq 'OttrinIndexQA') 'Wrong account'
 $id=[Security.Principal.WindowsIdentity]::GetCurrent()
 Assert (-not (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Elevated actor'
 Assert ($prior.status -eq 'pass') 'Prior verification failed'
 $install=Join-Path $env:LOCALAPPDATA 'Programs\Ottrin';$app=Join-Path $install 'bin\ottrin.exe'
 $db=Join-Path $env:LOCALAPPDATA 'ottrin\search.db'
 $pref=Join-Path $env:APPDATA 'ottrin\config.json';$source=Join-Path $env:USERPROFILE 'OttrinFixtures\scheduled-daemon-fixture.txt'
 $taskName='Ottrin Index '+$id.User.Value
 $result.service_remove=Invoke-App 'index service-remove'
 $scheduler=New-Object -ComObject Schedule.Service;$scheduler.Connect();$folder=$scheduler.GetFolder('\')
 $absent=$false;try{$null=$folder.GetTask($taskName)}catch{if($_.Exception.HResult -eq -2147024894){$absent=$true}else{throw}}
 Assert $absent 'Task remains after removal'
 Assert (Test-Path $db) 'Service removal deleted index'
 $result.database_retained_after_service_remove=$true
 $p=Start-Process "$install\Uninstall.exe" -ArgumentList '/S' -PassThru
 $null=$p.Handle;Assert ($p.WaitForExit(30000)) 'Uninstaller bootstrap timeout'
 Assert ($p.ExitCode -eq 0) "Uninstaller bootstrap exit $($p.ExitCode)"
 $result.bootstrap_exit=$p.ExitCode
 $deadline=[DateTime]::UtcNow.AddSeconds(30)
 while((Test-Path $install) -and [DateTime]::UtcNow -lt $deadline){Start-Sleep -Milliseconds 200}
 Assert (-not (Test-Path $install)) 'Install directory remains'
 Assert (-not (Test-Path $db)) 'Generated index remains'
 Assert (-not (Test-Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Ottrin')) 'Installed Apps key remains'
 Assert (-not (Test-Path (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Ottrin'))) 'Start menu entry remains'
 Assert ((Get-FileHash $pref).Hash -eq $prior.preference_hash_before) 'Preferences changed'
 Assert ((Get-FileHash $source).Hash -eq $prior.source_hash_before) 'Source changed'
 $result.preference_hash=(Get-FileHash $pref).Hash;$result.source_hash=(Get-FileHash $source).Hash
 $result.installed_files_removed=$true;$result.generated_index_removed=$true;$result.task_removed=$true
 $result.phase='uninstalled';$result.status='pass'
} catch {$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
$result|ConvertTo-Json -Depth 10|Set-Content "$r\removed.json" -Encoding UTF8
if($result.status -ne 'pass'){exit 1}
exit 0
