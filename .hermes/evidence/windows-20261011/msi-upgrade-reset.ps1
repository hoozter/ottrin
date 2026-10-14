$ProgressPreference='SilentlyContinue';$ErrorActionPreference='Stop'
$b='C:\OttrinQA-20261011';$r="$b\msi-upgrade-tests"
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
try {
 Assert ($env:USERNAME -eq 'OttrinIndexQA') 'Wrong account'
 $prior=Get-Content "$r\result.json" -Raw|ConvertFrom-Json
 $installer=New-Object -ComObject WindowsInstaller.Installer
 foreach($code in @($prior.new_product_code,$prior.old_product_code)){
  $state=$installer.ProductState($code)
  Assert ($state -in @(5,-1)) "Unexpected product state: $code"
  if($state -eq -1){continue}
  $null=Invoke-Msi "/x $code /qn /norestart /l*v $r\reset-$code.log"
  Assert ($installer.ProductState($code) -eq -1) "Product remains: $code"
 }
 Assert (-not(Test-Path (Join-Path $env:LOCALAPPDATA 'Programs\Ottrin'))) 'App remains'
 Assert ((Get-FileHash (Join-Path $env:APPDATA 'ottrin\config.json')).Hash -eq $prior.preference_hash_before) 'Preferences changed'
 Assert ((Get-FileHash (Join-Path $env:USERPROFILE 'OttrinFixtures\scheduled-daemon-fixture.txt')).Hash -eq $prior.source_hash_before) 'Source changed'
 $result.status='pass';$result.phase='reset'
}catch{$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
$result|ConvertTo-Json -Depth 10|Set-Content "$r\reset.json" -Encoding UTF8
if($result.status -ne 'pass'){exit 1};exit 0
