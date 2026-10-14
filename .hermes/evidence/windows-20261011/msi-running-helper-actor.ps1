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
try {
 Assert ($env:USERNAME -eq 'OttrinIndexQA') 'Wrong account'
 $id=[Security.Principal.WindowsIdentity]::GetCurrent()
 Assert (-not (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Elevated actor'
 $install=Join-Path $env:LOCALAPPDATA 'Programs\Ottrin';$app=Join-Path $install 'bin\ottrin.exe'
 $baseline=Get-Content "$r\result.json" -Raw|ConvertFrom-Json
 $installer=New-Object -ComObject WindowsInstaller.Installer
 $result.product_state_before=$installer.ProductState($baseline.product_code)
 Assert ($result.product_state_before -eq 1) 'Expected captured advertised rollback state'
 $scripts=Join-Path $r 'scripts';New-Item $scripts -ItemType Directory|Out-Null
 Copy-Item "$b\rollback-new-identity.ps1" "$scripts\windows-index-identity.ps1"
 Copy-Item "$b\rollback-new-transaction.ps1" "$scripts\index-task-transaction.ps1"
 . "$scripts\windows-index-identity.ps1"
 $scheduler=New-Object -ComObject Schedule.Service;$scheduler.Connect();$folder=$scheduler.GetFolder('\');$name=Get-OttrinIndexTaskName
 $results=@()
 foreach($case in @('enabled-running','disabled-running','enabled-idle','unchanged-running')){
  $result.current_case=$case
  $task=$folder.GetTask($name)
  $task.Enabled=$true
  if($case -eq 'enabled-idle'){
   $peers=@(Get-CimInstance Win32_Process -Filter "Name='ottrin-indexd.exe'"|Where-Object {$_.ExecutablePath -eq (Join-Path $install 'bin\ottrin-indexd.exe')}|ForEach-Object {[Diagnostics.Process]::GetProcessById($_.ProcessId)})
   Stop-OttrinIndexTask $task
   foreach($peer in $peers){try{Assert ($peer.WaitForExit(10000)) 'Owned daemon still exiting'}finally{$peer.Dispose()}}
  }else{
   $null=Invoke-App 'index service-restart'
   $task=$folder.GetTask($name)
   $deadline=[DateTime]::UtcNow.AddSeconds(20)
   while($task.GetInstances(0).Count -ne 1 -and [DateTime]::UtcNow -lt $deadline){Start-Sleep -Milliseconds 50}
   Assert ($task.GetInstances(0).Count -eq 1) 'Task did not start before snapshot'
  }
  if($case -eq 'disabled-running'){$task.Enabled=$false}
  $task=$folder.GetTask($name)
  $beforeInstance=$(if($task.GetInstances(0).Count){$task.GetInstances(0).Item(1).InstanceGuid}else{$null})
  $beforeXml=$task.Xml;$result.before_xml=$beforeXml;$beforeSecurity=$task.GetSecurityDescriptor(7);$wasRunning=$task.GetInstances(0).Count -ne 0
  $snapshot=Join-Path $r "$case.json"
  & "$scripts\index-task-transaction.ps1" -Phase Snapshot -Snapshot $snapshot
  $record=Get-Content $snapshot -Raw|ConvertFrom-Json
  Assert ($record.version -eq 3 -and $record.tasks[0].running -eq $wasRunning) 'Running state not captured'
  if($case -ne 'unchanged-running'){$null=Invoke-App 'index service-remove'}
  & "$scripts\index-task-transaction.ps1" -Phase Rollback -Snapshot $snapshot
  $after=$folder.GetTask($name)
  $deadline=[DateTime]::UtcNow.AddSeconds(20)
  while(($after.GetInstances(0).Count -ne [int]$wasRunning) -and [DateTime]::UtcNow -lt $deadline){Start-Sleep -Milliseconds 50}
  Assert ($after.GetInstances(0).Count -eq [int]$wasRunning) 'Rollback did not restore running state'
$result.after_xml=$after.Xml
  Assert ($after.Xml -eq $beforeXml) 'Rollback changed definition/enabled state'
  if($case -eq 'unchanged-running'){Assert ($after.GetInstances(0).Item(1).InstanceGuid -eq $beforeInstance) 'Unchanged task instance replaced'}
  Assert ($after.GetSecurityDescriptor(7) -eq $beforeSecurity) 'Rollback changed security'
  Assert (-not(Test-Path $snapshot) -and -not(Test-Path "$snapshot.scripts")) 'Transaction artifacts remain'
  $results+=@{case=$case;running_before=$wasRunning;instances_after=$after.GetInstances(0).Count;enabled_after=$after.Enabled;xml_preserved=$true;security_preserved=$true}
  $result.cases=$results
 }
 $result.cases=$results
 $folder.GetTask($name).Enabled=$true
 $null=Invoke-App 'index service-restart'
 $result.query=(Invoke-App 'search scheduled-daemon-fixture --json')|ConvertFrom-Json
 Assert ($result.query.total -eq 1) 'Query did not recover'
 $result.status='pass';$result.phase='helper-verified'
}catch{$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
$result|ConvertTo-Json -Depth 30|Set-Content "$r\helper.json" -Encoding UTF8
if($result.status -ne 'pass'){exit 1};exit 0
