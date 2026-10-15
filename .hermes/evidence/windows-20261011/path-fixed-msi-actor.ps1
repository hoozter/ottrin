$ProgressPreference='SilentlyContinue';$ErrorActionPreference='Stop'
$b='C:\OttrinQA-20261011';$r="$b\msi-path-current-lifecycle"
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
 $id=[Security.Principal.WindowsIdentity]::GetCurrent()
 Assert (-not (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Elevated actor'
 $install=Join-Path $env:LOCALAPPDATA 'Programs\Ottrin';$app=Join-Path $install 'bin\ottrin.exe'
 Assert (-not (Test-Path $install)) 'Unexpected existing app'
 $pref=Join-Path $env:APPDATA 'ottrin\config.json';$source=Join-Path $env:USERPROFILE 'OttrinFixtures\scheduled-daemon-fixture.txt'
 $prefHash=(Get-FileHash $pref).Hash;$sourceHash=(Get-FileHash $source).Hash
 $result.preference_hash_before=$prefHash;$result.source_hash_before=$sourceHash
 $result.msi_hash=(Get-FileHash "$b\msi-final.msi").Hash.ToLower()
 Assert ($result.msi_hash -eq 'de47dae2de001294809187d91cb981d1e3b8aa6a542f5369045453b5807421f9') 'Wrong MSI'
 $installer=New-Object -ComObject WindowsInstaller.Installer
 $db=$installer.OpenDatabase("$b\msi-final.msi",0)
 $view=$db.OpenView("SELECT ``Value`` FROM ``Property`` WHERE ``Property``='ProductCode'")
 $view.Execute();$record=$view.Fetch();$code=$record.StringData(1);$view.Close()
 $result.product_code=$code
 $result.install_exit=Invoke-Msi "/i $b\msi-final.msi /qn /norestart /l*v $r\install.log"
 Assert ($installer.ProductState($code) -eq 5) 'Product not fully installed'
 $hashes=Get-Content "$b\msi-candidate-hashes.json" -Raw|ConvertFrom-Json
 foreach($p in $hashes.PSObject.Properties){Assert ((Get-FileHash (Join-Path $install $p.Name)).Hash.ToLower() -eq $p.Value) "Payload differs: $($p.Name)"}
 $result.payload_files=@($hashes.PSObject.Properties).Count
 $scheduler=New-Object -ComObject Schedule.Service;$scheduler.Connect()
 $taskName='Ottrin Index '+$id.User.Value;$task=$scheduler.GetFolder('\').GetTask($taskName)
 Assert ($task.State -eq 4) 'Index task not running'
 Assert ($task.Definition.Actions.Item(1).Arguments -eq '--index-daemon') 'Wrong task action'
 Assert ($task.Definition.Triggers.Count -eq 1 -and $task.Definition.Triggers.Item(1).Type -eq 9) 'Wrong trigger'
 $result.task_xml=$task.Xml
 $result.index_update=Invoke-App 'index update --json'
 $result.query=(Invoke-App 'search scheduled-daemon-fixture --json')|ConvertFrom-Json
 Assert ($result.query.total -eq 1) 'Expected one result'
 Assert ((Get-FileHash $pref).Hash -eq $prefHash) 'Install changed preferences'
 $result.phase='uninstall'
 $result.uninstall_exit=Invoke-Msi "/x $code /qn /norestart /l*v $r\uninstall.log"
 Assert ($installer.ProductState($code) -eq -1) 'Product registration remains'
 Assert (-not (Test-Path $install)) 'Installed directory remains'
 Assert (-not (Test-Path (Join-Path $env:LOCALAPPDATA 'ottrin\search.db'))) 'Generated database remains'
 Assert (-not (Test-Path (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Ottrin'))) 'Start menu remains'
 $absent=$false;try{$null=$scheduler.GetFolder('\').GetTask($taskName)}catch{if($_.Exception.HResult -eq -2147024894){$absent=$true}else{throw}}
 Assert $absent 'Index task remains'
 Assert ((Get-FileHash $pref).Hash -eq $prefHash) 'Uninstall changed preferences'
 Assert ((Get-FileHash $source).Hash -eq $sourceHash) 'User source changed'
 $result.preference_hash_after=(Get-FileHash $pref).Hash;$result.source_hash_after=(Get-FileHash $source).Hash
 $result.phase='uninstalled';$result.status='pass'
}catch{$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
$result|ConvertTo-Json -Depth 30|Set-Content "$r\result.json" -Encoding UTF8
if($result.status -ne 'pass'){exit 1}
exit 0
