$ProgressPreference='SilentlyContinue';$ErrorActionPreference='Stop'
$b='C:\OttrinQA-20261011';$r="$b\index-upgrade-tests"
$loaded=Get-Content "$r\result.json" -Raw | ConvertFrom-Json
$result=@{};foreach($p in $loaded.PSObject.Properties){$result[$p.Name]=$p.Value}
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
 Assert ($result.old_payload_files -eq 51 -and $result.new_payload_files -eq 51 -and $result.upgrade_exit -eq 0) 'Upgrade prerequisites failed'
 $install=Join-Path $env:LOCALAPPDATA 'Programs\Ottrin';$app=Join-Path $install 'bin\ottrin.exe'
 $result.update=Invoke-App 'index update --json'
 $result.query=(Invoke-App 'search scheduled-daemon-fixture --json')|ConvertFrom-Json
 Assert ($result.query.total -eq 1) 'Expected one fixture result'
 $result.service_status=Invoke-App 'index service-status'
 $pref=Join-Path $env:APPDATA 'ottrin\config.json'
 $source=Join-Path $env:USERPROFILE 'OttrinFixtures\scheduled-daemon-fixture.txt'
 Assert ((Get-FileHash $pref).Hash -eq $result.preference_hash_before) 'Preferences changed'
 Assert ((Get-FileHash $source).Hash -eq $result.source_hash_before) 'Source changed'
 $result.preference_hash=(Get-FileHash $pref).Hash
 $result.source_hash=(Get-FileHash $source).Hash
 $result.phase='upgraded';$result.status='pass';$result.error=$null;$result.stack=$null
} catch {$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
$result | ConvertTo-Json -Depth 30 | Set-Content "$r\verified.json" -Encoding UTF8
if($result.status -ne 'pass'){exit 1}
exit 0
