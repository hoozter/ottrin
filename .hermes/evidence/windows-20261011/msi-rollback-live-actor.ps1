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
$lock=$null
try {
 Assert ($env:USERNAME -eq 'OttrinIndexQA') 'Wrong account'
 $id=[Security.Principal.WindowsIdentity]::GetCurrent()
 Assert (-not (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Elevated actor'
 $install=Join-Path $env:LOCALAPPDATA 'Programs\Ottrin';$app=Join-Path $install 'bin\ottrin.exe'
 Assert (-not(Test-Path $install)) 'Unexpected existing app'
 $pref=Join-Path $env:APPDATA 'ottrin\config.json';$source=Join-Path $env:USERPROFILE 'OttrinFixtures\scheduled-daemon-fixture.txt'
 $result.preference_hash_before=(Get-FileHash $pref).Hash;$result.source_hash_before=(Get-FileHash $source).Hash
 $installer=New-Object -ComObject WindowsInstaller.Installer;$db=$installer.OpenDatabase("$b\msi-rollback-current.msi",0)
 $view=$db.OpenView("SELECT ``Value`` FROM ``Property`` WHERE ``Property``='ProductCode'")
 $null=$view.Execute();$code=$view.Fetch().StringData(1);$null=$view.Close();$result.product_code=$code
 $result.msi_sha256=(Get-FileHash "$b\msi-rollback-current.msi").Hash.ToLower()
 $result.install_exit=Invoke-Msi "/i $b\msi-rollback-current.msi /qn /norestart /l*v $r\install.log"
 Assert ($installer.ProductState($code) -eq 5) 'Product not installed'
 $result.index_update=Invoke-App 'index update --json'
 $scheduler=New-Object -ComObject Schedule.Service;$scheduler.Connect();$folder=$scheduler.GetFolder('\')
 $taskName='Ottrin Index '+$id.User.Value;$before=$folder.GetTask($taskName)
 $result.task_name=$taskName;$result.task_before_xml=$before.Xml;$result.task_before_security=$before.GetSecurityDescriptor(7)
 $result.instances_before=$before.GetInstances(0).Count
 Assert ($result.instances_before -eq 1) 'Expected running scheduled daemon'
 $cache=Join-Path $env:LOCALAPPDATA 'ottrin\preview_cache';New-Item $cache -ItemType Directory -Force|Out-Null
 $file=Join-Path $cache 'rollback-owned-locked';[IO.File]::WriteAllText($file,'OWNED ROLLBACK CACHE')
 $lock=[IO.File]::Open($file,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
 $result.phase='locked-uninstall'
 $p=Start-Process msiexec.exe -ArgumentList "/x $code /qn /norestart /l*v $r\failure.log" -PassThru
 $null=$p.Handle;Assert ($p.WaitForExit(120000)) 'Uninstall timeout'
 $result.failed_uninstall_exit=$p.ExitCode;Assert ($p.ExitCode -eq 1603) 'Expected cleanup failure'
 $result.product_state_after_failure=$installer.ProductState($code)
 $after=$folder.GetTask($taskName)
 $result.task_after_xml=$after.Xml;$result.task_after_security=$after.GetSecurityDescriptor(7)
 $result.instances_after=$after.GetInstances(0).Count;$result.task_state_after=$after.State
 $hashes=Get-Content "$b\msi-rollback-current-hashes.json" -Raw|ConvertFrom-Json
 foreach($h in $hashes.PSObject.Properties){Assert ((Get-FileHash (Join-Path $install $h.Name)).Hash.ToLower() -eq $h.Value) "Payload differs: $($h.Name)"}
 $result.payload_files=@($hashes.PSObject.Properties).Count
 $result.preference_hash_after=(Get-FileHash $pref).Hash;$result.source_hash_after=(Get-FileHash $source).Hash
 $result.database_after_failure=Test-Path (Join-Path $env:LOCALAPPDATA 'ottrin\search.db')
 Assert ($result.task_before_xml -eq $result.task_after_xml) 'Task definition not restored'
 Assert ($result.task_before_security -eq $result.task_after_security) 'Task security not restored'
 Assert ($result.preference_hash_before -eq $result.preference_hash_after -and $result.source_hash_before -eq $result.source_hash_after) 'Preferences/source changed'
 $result.observation_complete=$true
 $result.status='rollback-incomplete';$result.phase='observed'
}catch{$result.status='fixture-failed';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally{if($lock){$lock.Dispose()}}
$result|ConvertTo-Json -Depth 30|Set-Content "$r\result.json" -Encoding UTF8
if($result.status -eq 'fixture-failed'){exit 1};exit 0
