$ProgressPreference='SilentlyContinue';$ErrorActionPreference='Stop'
$b='C:\OttrinQA-20261011';$r="$b\msi-upgrade-tests"
$result=@{status='running';phase='downgrade'}
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
function Product-Code($path){
 $db=$script:installer.OpenDatabase($path,0)
 $view=$db.OpenView("SELECT ``Value`` FROM ``Property`` WHERE ``Property``='ProductCode'")
 $null=$view.Execute();$record=$view.Fetch();$code=$record.StringData(1);$null=$view.Close();return $code
}
function Check-Payload($name){
 $hashes=Get-Content "$b\$name" -Raw|ConvertFrom-Json
 foreach($p in $hashes.PSObject.Properties){Assert ((Get-FileHash (Join-Path $script:install $p.Name)).Hash.ToLower() -eq $p.Value) "Payload differs: $($p.Name)"}
 return @($hashes.PSObject.Properties).Count
}
try {
 Assert ($env:USERNAME -eq 'OttrinIndexQA') 'Wrong account'
 $id=[Security.Principal.WindowsIdentity]::GetCurrent()
 Assert (-not (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Elevated actor'
 $prior=Get-Content "$r\result.json" -Raw|ConvertFrom-Json;Assert ($prior.status -eq 'pass') 'Upgrade prerequisite failed'
 $install=Join-Path $env:LOCALAPPDATA 'Programs\Ottrin';$app=Join-Path $install 'bin\ottrin.exe'
 $installer=New-Object -ComObject WindowsInstaller.Installer
 $code=Product-Code "$b\msi-downgrade-fixture.msi"
 $result.fixture_product_code=$code;$result.fixture_version='0.0.9';$result.fixture_sha256=(Get-FileHash "$b\msi-downgrade-fixture.msi").Hash.ToLower()
 $p=Start-Process msiexec.exe -ArgumentList "/i $b\msi-downgrade-fixture.msi /qn /norestart /l*v $r\downgrade.log" -PassThru
 $null=$p.Handle;Assert ($p.WaitForExit(120000)) 'Downgrade CLI timeout'
 $result.downgrade_exit=$p.ExitCode
 Assert ($p.ExitCode -eq 1603) 'Downgrade not rejected'
 $log=Get-Content "$r\downgrade.log" -Raw
 Assert ($log -match 'A newer version of Ottrin is already installed') 'Wrong failure cause'
 Assert ($installer.ProductState($prior.new_product_code) -eq 5) 'Current registration changed'
 Assert ($installer.ProductState($code) -eq -1) 'Lower fixture registered'
 $result.payload_files=Check-Payload 'msi-upgrade-new-hashes.json'
 Assert ((Get-FileHash (Join-Path $env:APPDATA 'ottrin\config.json')).Hash -eq $prior.preference_hash_before) 'Preferences changed'
 Assert ((Get-FileHash (Join-Path $env:USERPROFILE 'OttrinFixtures\scheduled-daemon-fixture.txt')).Hash -eq $prior.source_hash_before) 'Source changed'
 $result.query=(Invoke-App 'search scheduled-daemon-fixture --json')|ConvertFrom-Json
 Assert ($result.query.total -eq 1) 'Search changed'
 $result.status='pass';$result.phase='downgrade-blocked'
}catch{$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
$result|ConvertTo-Json -Depth 30|Set-Content "$r\downgrade.json" -Encoding UTF8
if($result.status -ne 'pass'){exit 1};exit 0
