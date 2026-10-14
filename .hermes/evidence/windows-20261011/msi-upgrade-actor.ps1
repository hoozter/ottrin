param([switch]$Resume)
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
 $install=Join-Path $env:LOCALAPPDATA 'Programs\Ottrin';$app=Join-Path $install 'bin\ottrin.exe'
 if($Resume){
  Assert (Test-Path $install) 'Resume has no installed app'
  $prior=Get-Content "$r\result.json" -Raw|ConvertFrom-Json
 }else{Assert (-not (Test-Path $install)) 'Unexpected existing app'}
 $pref=Join-Path $env:APPDATA 'ottrin\config.json';$source=Join-Path $env:USERPROFILE 'OttrinFixtures\scheduled-daemon-fixture.txt'
 $prefHash=(Get-FileHash $pref).Hash;$sourceHash=(Get-FileHash $source).Hash
 if($Resume){Assert ($prefHash -eq $prior.preference_hash_before -and $sourceHash -eq $prior.source_hash_before) 'Resume baseline differs'}
 $result.preference_hash_before=$prefHash;$result.source_hash_before=$sourceHash
 $installer=New-Object -ComObject WindowsInstaller.Installer
 $oldCode=Product-Code "$b\msi-upgrade-old.msi";$newCode=Product-Code "$b\msi-upgrade-new.msi"
 $result.old_product_code=$oldCode;$result.new_product_code=$newCode
 $result.old_msi_hash=(Get-FileHash "$b\msi-upgrade-old.msi").Hash.ToLower();$result.new_msi_hash=(Get-FileHash "$b\msi-upgrade-new.msi").Hash.ToLower()
 Assert ($oldCode -ne $newCode) 'Products have same code'
 if($Resume){$result.old_install_exit=$prior.old_install_exit}else{$result.old_install_exit=Invoke-Msi "/i $b\msi-upgrade-old.msi /qn /norestart /l*v $r\old-install.log"}
 Assert ($installer.ProductState($oldCode) -eq 5) 'Old product not installed'
 $result.old_payload_files=Check-Payload 'msi-upgrade-old-hashes.json'
 $scheduler=New-Object -ComObject Schedule.Service;$scheduler.Connect();$folder=$scheduler.GetFolder('\')
 $taskName='Ottrin Index '+$id.User.Value;$oldTask=$folder.GetTask($taskName)
 Assert ($oldTask.Definition.Actions.Item(1).Arguments -eq '--index-update') 'Old task not periodic'
 $result.old_task_xml=$oldTask.Xml
 $result.old_index_update=Invoke-App 'index update --json'
 $result.old_query=(Invoke-App 'search scheduled-daemon-fixture --json')|ConvertFrom-Json
 Assert ($result.old_query.total -eq 1) 'Old fixture query not one'
 $result.phase='upgrade'
 $result.upgrade_exit=Invoke-Msi "/i $b\msi-upgrade-new.msi /qn /norestart /l*v $r\upgrade.log"
 $result.old_product_state_after_upgrade=$installer.ProductState($oldCode)
 $result.new_product_state_after_upgrade=$installer.ProductState($newCode)
 $result.new_payload_files=Check-Payload 'msi-upgrade-new-hashes.json'
 $newTask=$folder.GetTask($taskName)
 $result.new_task_xml=$newTask.Xml
 Assert ($result.old_product_state_after_upgrade -eq -1) 'Old product registration remains after upgrade'
 Assert ($result.new_product_state_after_upgrade -eq 5) 'New product not fully installed'
 Assert ($newTask.State -eq 4 -and $newTask.Definition.Actions.Item(1).Arguments -eq '--index-daemon') 'Persistent task not running'
 Assert ($newTask.Definition.Triggers.Count -eq 1 -and $newTask.Definition.Triggers.Item(1).Type -eq 9) 'Task not login trigger'
 $result.index_update=Invoke-App 'index update --json'
 $result.query=(Invoke-App 'search scheduled-daemon-fixture --json')|ConvertFrom-Json
 Assert ($result.query.total -eq 1) 'Upgraded query not one'
 Assert ((Get-FileHash $pref).Hash -eq $prefHash) 'Upgrade changed preferences'
 Assert ((Get-FileHash $source).Hash -eq $sourceHash) 'Upgrade changed source'
 $result.preference_hash_after=(Get-FileHash $pref).Hash;$result.source_hash_after=(Get-FileHash $source).Hash
 $result.phase='upgraded';$result.status='pass'
}catch{$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
$result|ConvertTo-Json -Depth 30|Set-Content "$r\result.json" -Encoding UTF8
if($result.status -ne 'pass'){exit 1}
exit 0
