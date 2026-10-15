$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$b='C:\OttrinFullRecoveryQA';$r="$b\results";$install="$env:LOCALAPPDATA\Programs\Ottrin";$app="$install\bin\ottrin.exe";$controller="$b\controller.exe";$fixture="$env:USERPROFILE\OttrinFixtures";$pref="$env:APPDATA\ottrin\config.json"
$active=@();$result=@{status='running';phase='prepare';cases=@()};$observed=$null;$observedTask=$null;$interruptedRoot=$null
function Assert($v,$m){if(-not$v){throw $m}}
function Phase($name){$result.phase=$name;$result|ConvertTo-Json -Depth 12|Set-Content "$r\PROGRESS.json"}
function StartOwned($program,$arguments){
 $s=New-Object Diagnostics.ProcessStartInfo($program,$arguments);$s.UseShellExecute=$false;$s.RedirectStandardOutput=$true;$s.RedirectStandardError=$true;$s.CreateNoWindow=$true
 $p=[Diagnostics.Process]::Start($s);$job=@{process=$p;out=$p.StandardOutput.ReadToEndAsync();err=$p.StandardError.ReadToEndAsync()};$script:active+=,$job;return $job
}
function StopTree($job){
 $p=$job.process
 if(-not$p.HasExited){& "$env:WINDIR\System32\taskkill.exe" /PID $p.Id /T /F|Out-Null;Assert ($LASTEXITCODE-eq0) 'Cannot stop owned installer tree';Assert ($p.WaitForExit(10000)) 'Owned process remains after tree termination'}
}
function SharedJson($path){
 # Delete-sharing is essential: observation must not block journal replacement.
 $f=[IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
 try{$reader=New-Object IO.StreamReader($f,[Text.Encoding]::UTF8);try{return ($reader.ReadToEnd()|ConvertFrom-Json)}finally{$reader.Dispose()}}finally{$f.Dispose()}
}
function Observe {
 try{
  $v=SharedJson "$install.ottrin-setup\transaction.json"
  if($v.root-cne$script:interruptedRoot){
   if($v.state-eq'staging'){$script:observed=$v}
   if($null-eq$script:observedTask -and $v.state-in@('staging','integrating','publishing','published')){
    try{$script:observedTask=SharedJson "$install.ottrin-setup\index-transaction.json"}catch [IO.IOException]{}catch [ArgumentException]{}
   }
  }
 }catch [IO.IOException]{}catch [ArgumentException]{}
}
function WaitOwned($job,$observe=$false){
 $p=$job.process;$deadline=[DateTime]::UtcNow.AddSeconds(180)
 while(-not$p.WaitForExit(30)){if($observe){Observe};Assert ([DateTime]::UtcNow-lt$deadline) "Owned process timeout PID $($p.Id)"}
 if($observe){Observe}
 Assert ($job.out.Wait(5000)-and$job.err.Wait(5000)) 'Owned output remains open'
 return @{exit=$p.ExitCode;stdout=$job.out.Result;stderr=$job.err.Result}
}
function Run($program,$arguments){$job=StartOwned $program $arguments;try{$v=WaitOwned $job;Assert ($v.exit-eq0) "Process error $($v.exit): $arguments : $($v.stderr)";return $v}finally{StopTree $job}}
function Task { $folder.GetTask($taskName) }
function CountInstances { (Task).GetInstances(0).Count }
function CheckData {
 Assert ((Get-FileHash $pref).Hash-ceq$prefHash) 'Preferences changed'
 Assert ((Get-FileHash "$fixture\upgrade-source.txt").Hash-ceq$sourceHash) 'Source changed'
 $v=(Run $app 'search upgrade-source --json').stdout|ConvertFrom-Json;Assert ($v.total-eq1) 'Saved index query lost'
}
function VerifyPayload {
 foreach($e in $manifest.PSObject.Properties){Assert ((Get-FileHash (Join-Path $install $e.Name)).Hash.ToLowerInvariant()-ceq$e.Value) "Payload mismatch: $($e.Name)"}
 Assert (@(Get-ChildItem $install -Recurse -File).Count-eq(@($manifest.PSObject.Properties).Count+1)) 'Unexpected installed payload count'
 Assert (-not(Test-Path "$install.ottrin-setup")) 'Active payload record remains'
 Assert (-not(Test-Path "$install.ottrin-completed")) 'Completed record remains'
}
try{
 Assert ($env:USERNAME-eq'OttrinFullRecoveryQA') 'Wrong actor account'
 $id=[Security.Principal.WindowsIdentity]::GetCurrent();Assert (-not(New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Actor is elevated'
 $sid=$id.User.Value;$service=New-Object -ComObject Schedule.Service;$service.Connect();$folder=$service.GetFolder('\');$taskName='Ottrin Index '+$sid
 Assert (-not(Test-Path $install)) 'Unexpected existing own installation'
 New-Item -ItemType Directory $fixture|Out-Null;[IO.File]::WriteAllText("$fixture\upgrade-source.txt",'Owned full payload restart fixture')
 $config=Get-Content "$b\config-template.json" -Raw|ConvertFrom-Json;$config.bookmarks=@();$config.last_session_dirs=@();$config.search.include_roots=@($fixture);$config.search.exclude_roots=@();$config.search.content_indexing_enabled=$false
 $null=[IO.Directory]::CreateDirectory((Split-Path $pref));[IO.File]::WriteAllText($pref,($config|ConvertTo-Json -Depth 50),[Text.UTF8Encoding]::new($false))
 $prefHash=(Get-FileHash $pref).Hash;$sourceHash=(Get-FileHash "$fixture\upgrade-source.txt").Hash;$manifest=Get-Content "$b\payload.json" -Raw|ConvertFrom-Json
 $definition=[regex]::Match((Get-Content "$b\payload-transaction.qa.ps1" -Raw),"(?s)Add-Type -TypeDefinition @'\r?\n(.*?)\r?\n'@").Groups[1].Value;Assert ($definition.Length-gt100) 'Missing identity type';Add-Type -TypeDefinition $definition
 foreach($case in @(@{point='old-moved';enabled=$true;running=$true},@{point='candidate-moved';enabled=$false;running=$true},@{point='rolled-back';enabled=$true;running=$false},@{point='committed';enabled=$false;running=$false})){
  Phase ("baseline-"+$case.point);$null=Run "$b\baseline.exe" '/S';$null=Run $app 'index update --json';CheckData
  $oldId=[OttrinInstallIdentity]::Read($install);$db="$env:LOCALAPPDATA\ottrin\search.db";$dbCreated=[IO.File]::GetCreationTimeUtc($db).Ticks
  $t=Task;$t.Enabled=$case.enabled;if(-not$case.running){$null=Run $controller '--stop'}
  Assert (((CountInstances)-gt0)-eq$case.running) 'Baseline running state incorrect'
  $oldXml=(Task).Xml;$oldSecurity=(Task).GetSecurityDescriptor(7)
  # Typed values verify the pre-commit integration snapshot, not just files.
  $key=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\Microsoft\Windows\CurrentVersion\Uninstall\Ottrin',$true)
  try{$key.SetValue('NoModify',[byte[]]@(0,255),[Microsoft.Win32.RegistryValueKind]::Binary);$key.SetValue('NoRepair',[long]4294967297,[Microsoft.Win32.RegistryValueKind]::QWord)}finally{$key.Dispose()}
  $env:OTTRIN_QA_PAUSE_AT=$case.point;$env:OTTRIN_QA_CHECKPOINT_FILE="$r\checkpoint-$($case.point).json"
  $setup=if($case.point-eq'rolled-back'){"$b\rollback.exe"}else{"$b\pause.exe"}
  Phase ("interrupt-"+$case.point);$job=StartOwned $setup '/S';$deadline=[DateTime]::UtcNow.AddSeconds(90)
  while(-not(Test-Path $env:OTTRIN_QA_CHECKPOINT_FILE)){Assert (-not$job.process.HasExited) "Setup exited before $($case.point)";Assert ([DateTime]::UtcNow-lt$deadline) 'Checkpoint timed out';Start-Sleep -Milliseconds 50}
  $checkpoint=SharedJson $env:OTTRIN_QA_CHECKPOINT_FILE;StopTree $job;$env:OTTRIN_QA_PAUSE_AT=$null
  Assert ($checkpoint.point-ceq$case.point) 'Wrong interruption checkpoint'
  $interruptedRoot=$checkpoint.transaction.root;$committed=$case.point-eq'committed'
  Assert ((CountInstances)-eq0) 'Unexpected running task at selected stopped checkpoint'
  Assert (@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue|Where-Object {$_.Path-eq"$install\bin\ottrin-indexd.exe"}).Count-eq0) 'Installed daemon remained live at selected stopped checkpoint'
  $expectedXml=if($committed){(Task).Xml}else{$oldXml};$expectedSecurity=if($committed){(Task).GetSecurityDescriptor(7)}else{$oldSecurity}
  $observed=$null;$observedTask=$null
  Phase ("retry-"+$case.point);$retry=StartOwned "$b\current.exe" '/S'
  try{$v=WaitOwned $retry $true;Assert ($v.exit-eq0) "Exact retry failed: $($v.stderr)"}finally{StopTree $retry}
  Assert ($null-ne$observed) 'New transaction was not observed after recovery'
  $expectedRoot=if($committed){$checkpoint.transaction.candidate}else{$oldId}
  Assert ($observed.previous-ceq$expectedRoot) 'Recovery chose incorrect original/committed root'
  Assert ($observedTask.version-eq3-and$observedTask.user-ceq$sid) 'Missing authenticated user task snapshot in retry'
  $taskRecord=@($observedTask.tasks|Where-Object name -eq $taskName);Assert ($taskRecord.Count-eq1) 'Retry task snapshot is incomplete'
  Assert ($taskRecord[0].running-eq$case.running) 'Retry prepared incorrect restored running intent'
  Assert ($taskRecord[0].xml-ceq$expectedXml) 'Retry did not capture the recovered task definition'
  Assert ($taskRecord[0].security-ceq$expectedSecurity) 'Retry did not capture recovered task permissions'
  $ownedKey=@($observed.registry|Where-Object path -like '*Uninstall\Ottrin')[0];$modified=@($ownedKey.values|Where-Object name -eq 'NoModify')[0]
  Assert ($modified.kind-eq$(if($committed){4}else{3})) 'Recovery chose wrong typed integration snapshot'
  if($committed){Assert ($modified.value-eq1) 'Committed registration value changed'}else{Assert ((@($modified.value)-join ',')-ceq'0,255') 'Original binary registration value not restored'}
  Assert ((Task).Enabled-eq$case.enabled) 'Final login intent changed';Assert (((CountInstances)-gt0)-eq$case.running) 'Final runtime intent changed'
  VerifyPayload;CheckData;Assert ([IO.File]::GetCreationTimeUtc($db).Ticks-eq$dbCreated) 'Saved index database replaced'
  $observed|ConvertTo-Json -Depth 14|Set-Content "$r\observed-$($case.point).json";$observedTask|ConvertTo-Json -Depth 8|Set-Content "$r\task-$($case.point).json"
  $result.cases+=@{point=$case.point;enabled=$case.enabled;running=$case.running;status='pass';old_root=$oldId;recovered_root=$observed.previous;committed_candidate_retained=$committed;payload_files=@($manifest.PSObject.Properties).Count;task_xml_security_and_runtime_preserved=$true;typed_registration_verified=$true;preferences_source_and_saved_index_preserved=$true}
  Phase ("uninstall-"+$case.point);$null=Run $controller '--stop';Copy-Item "$install\Uninstall.exe" "$r\uninstall.exe" -Force;$null=Run "$r\uninstall.exe" "/S _?=$install";Assert (-not(Test-Path $install)) 'Case uninstall incomplete';Assert ((Get-FileHash $pref).Hash-ceq$prefHash) 'Uninstall changed preferences'
 }
 $result.current_installer_sha256=(Get-FileHash "$b\current.exe").Hash.ToLowerInvariant();$result.status='pass';Phase 'accepted'
}catch{$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally{
 $env:OTTRIN_QA_PAUSE_AT=$null;$cleanupErrors=@()
 foreach($job in $active){try{StopTree $job}catch{$cleanupErrors+= $_.Exception.Message};$job.process.Dispose()}
 if($result.status-ne'pass'){
  try{$null=Run $controller '--stop'}catch{$cleanupErrors+=$_.Exception.Message}
  if($null-ne$folder){try{$t=Task;$t.Enabled=$false;$t.Stop(0)}catch{if($_.Exception.GetBaseException().HResult-ne-2147024894){$cleanupErrors+=$_.Exception.Message}}}
 }
 if($cleanupErrors.Count){$result.status='fail';$result.cleanup_errors=$cleanupErrors}
 $result|ConvertTo-Json -Depth 14|Set-Content "$r\RESULTS.json"
}
if($result.status-ne'pass'){exit 1}
