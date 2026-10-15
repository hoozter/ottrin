$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$b='C:\OttrinInitFinalQA';$r="$b\results";$install="$env:LOCALAPPDATA\Programs\Ottrin";$app="$install\bin\ottrin.exe";$controller="$b\controller.exe";$fixture="$env:USERPROFILE\OttrinFixtures";$pref="$env:APPDATA\ottrin\config.json"
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
function TaskAbsent {
 try{$null=Task;return $false}catch{if($_.Exception.GetBaseException().HResult-eq-2147024894){return $true};throw}
}
function Live {
 $t=Task;Assert ($t.GetInstances(0).Count-eq1) 'Expected one real task instance'
 $p=@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue|Where-Object {$_.Path-eq"$install\bin\ottrin-indexd.exe"});Assert ($p.Count-eq1) 'Expected one installed index daemon'
 return @{pid=$p[0].Id;instance=$t.GetInstances(0).Item(1).InstanceGuid}
}
function VerifyOldPayload {
 foreach($e in $oldManifest.PSObject.Properties){Assert ((Get-FileHash (Join-Path $install $e.Name)).Hash.ToLowerInvariant()-ceq$e.Value) "Interrupted baseline payload changed: $($e.Name)"}
}
function VerifyTypedRegistry {
 $key=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\Microsoft\Windows\CurrentVersion\Uninstall\Ottrin')
 try{Assert ($key.GetValueKind('NoModify')-eq[Microsoft.Win32.RegistryValueKind]::Binary) 'Original registry kind changed';Assert (($key.GetValue('NoModify')-join ',')-ceq'0,255') 'Original binary value changed';Assert ($key.GetValueKind('NoRepair')-eq[Microsoft.Win32.RegistryValueKind]::QWord-and$key.GetValue('NoRepair')-eq4294967297) 'Original QWord changed'}finally{$key.Dispose()}
}
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
 Assert ($env:USERNAME-eq'OttrinInitFinalQA') 'Wrong actor account'
 $id=[Security.Principal.WindowsIdentity]::GetCurrent();Assert (-not(New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Actor is elevated'
 $sid=$id.User.Value;$service=New-Object -ComObject Schedule.Service;$service.Connect();$folder=$service.GetFolder('\');$taskName='Ottrin Index '+$sid
 Assert (-not(Test-Path $install)) 'Own profile is not fresh'
 New-Item -ItemType Directory $fixture|Out-Null;[IO.File]::WriteAllText("$fixture\upgrade-source.txt",'Owned real payload initialization recovery fixture')
 $config=Get-Content "$b\config-template.json" -Raw|ConvertFrom-Json;$config.bookmarks=@();$config.last_session_dirs=@();$config.search.include_roots=@($fixture);$config.search.exclude_roots=@();$config.search.content_indexing_enabled=$false
 $null=[IO.Directory]::CreateDirectory((Split-Path $pref));[IO.File]::WriteAllText($pref,($config|ConvertTo-Json -Depth 50),[Text.UTF8Encoding]::new($false))
 $prefHash=(Get-FileHash $pref).Hash;$sourceHash=(Get-FileHash "$fixture\upgrade-source.txt").Hash
 $manifest=Get-Content "$b\payload.json" -Raw|ConvertFrom-Json;$oldManifest=Get-Content "$b\old-payload.json" -Raw|ConvertFrom-Json
 $definition=[regex]::Match((Get-Content "$b\payload-transaction.qa.ps1" -Raw),"(?s)Add-Type -TypeDefinition @'\r?\n(.*?)\r?\n'@").Groups[1].Value;Assert ($definition.Length-gt100) 'Missing identity implementation';Add-Type -TypeDefinition $definition
 $cases=@(@{name='root-fresh';point='root';existing=$false},@{name='new-fresh';point='new';existing=$false},@{name='root-enabled-running';point='root';existing=$true;enabled=$true;running=$true},@{name='root-disabled-stopped';point='root';existing=$true;enabled=$false;running=$false},@{name='new-disabled-running';point='new';existing=$true;enabled=$false;running=$true},@{name='new-enabled-stopped';point='new';existing=$true;enabled=$true;running=$false})
 foreach($case in $cases){
  $name=$case.name;Phase ('prepare-'+$name);Assert (TaskAbsent) 'Case begins with an index task';Assert (-not(Test-Path $install)) 'Case begins installed'
  $oldId=$null;$oldXml=$null;$oldSecurity=$null;$oldLive=$null;$dbId=$null
  if($case.existing){
   $null=Run "$b\baseline.exe" '/S';$null=Run $app 'index update --json';CheckData
   $oldId=[OttrinInstallIdentity]::Read($install);$db="$env:LOCALAPPDATA\ottrin\search.db";$dbId=[OttrinInstallIdentity]::Read($db)
   $t=Task;$t.Enabled=$case.enabled;if(-not$case.running){$null=Run $controller '--stop'}
   Assert (((CountInstances)-gt0)-eq$case.running) 'Baseline runtime is wrong'
   $oldXml=(Task).Xml;$oldSecurity=(Task).GetSecurityDescriptor(7);if($case.running){$oldLive=Live}
   $key=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\Microsoft\Windows\CurrentVersion\Uninstall\Ottrin',$true)
   try{$key.SetValue('NoModify',[byte[]]@(0,255),[Microsoft.Win32.RegistryValueKind]::Binary);$key.SetValue('NoRepair',[long]4294967297,[Microsoft.Win32.RegistryValueKind]::QWord)}finally{$key.Dispose()}
  }
  $env:OTTRIN_QA_PAUSE_AT=$case.point;$env:OTTRIN_QA_CHECKPOINT_FILE="$r\checkpoint-$name.json"
  Phase ('interrupt-'+$name);$job=StartOwned "$b\pause.exe" '/S';$deadline=[DateTime]::UtcNow.AddSeconds(90)
  while(-not(Test-Path $env:OTTRIN_QA_CHECKPOINT_FILE)){Assert (-not$job.process.HasExited) 'Setup exited before its checkpoint';Assert ([DateTime]::UtcNow-lt$deadline) 'No initialization checkpoint';Start-Sleep -Milliseconds 50}
  $checkpoint=SharedJson $env:OTTRIN_QA_CHECKPOINT_FILE;StopTree $job;$env:OTTRIN_QA_PAUSE_AT=$null
  Assert ($checkpoint.point-ceq$case.point) 'Wrong checkpoint'
  Assert (-not(Test-Path "$install.ottrin-setup\transaction.json")) 'Checkpoint is after the first journal'
  Assert ([OttrinInstallIdentity]::Read("$install.ottrin-setup")-ceq$checkpoint.transaction.root) 'Interrupted root identity changed'
  Assert ($checkpoint.transaction.previous-ceq$oldId) 'Initializer captured wrong original root'
  $plugin=Split-Path $checkpoint.plugin_script;Assert ((Get-FileHash "$plugin\index-controller.exe").Hash.ToLowerInvariant()-ceq$manifest.'bin/ottrin-indexd.exe') 'Checkpoint controller is not the actual payload daemon'
  Assert ((Get-FileHash $checkpoint.plugin_script).Hash-eq(Get-FileHash "$b\payload-transaction.qa.ps1").Hash) 'Checkpoint helper differs'
  if($case.existing){
   Assert ([OttrinInstallIdentity]::Read($install)-ceq$oldId) 'Original root moved before journal'
   VerifyOldPayload;VerifyTypedRegistry
   Assert ((Task).Xml-ceq$oldXml-and(Task).GetSecurityDescriptor(7)-ceq$oldSecurity) 'Early initialization changed task XML/security'
   Assert (((CountInstances)-gt0)-eq$case.running) 'Early initialization changed runtime'
   if($case.running){$now=Live;Assert ($now.pid-eq$oldLive.pid-and$now.instance-ceq$oldLive.instance) 'Early initialization restarted daemon'}
   Assert ([OttrinInstallIdentity]::Read($db)-ceq$dbId) 'Early initialization replaced saved index'
  }else{Assert (-not(Test-Path $install)-and(TaskAbsent)) 'Fresh initialization installed app/task'}
  Assert ((Get-FileHash $pref).Hash-ceq$prefHash-and(Get-FileHash "$fixture\upgrade-source.txt").Hash-ceq$sourceHash) 'Early initialization changed user data'
  $interruptedRoot=$checkpoint.transaction.root;$observed=$null;$observedTask=$null
  Phase ('retry-'+$name);$retry=StartOwned "$b\current.exe" '/S'
  try{$v=WaitOwned $retry $true;Assert ($v.exit-eq0) "Exact current retry failed: $($v.stderr)"}finally{StopTree $retry}
  $observed|ConvertTo-Json -Depth 14|Set-Content "$r\observed-$name.json";$observedTask|ConvertTo-Json -Depth 8|Set-Content "$r\task-$name.json"
  Assert ($null-ne$observed-and$observed.root-cne$interruptedRoot) 'New journal was not observed after empty-skeleton recovery'
  Assert ($observed.previous-ceq$oldId) 'Retry did not preserve original root'
  Assert ($observedTask.version-eq3-and$observedTask.user-ceq$sid) 'Retry task snapshot missing or wrong user'
  if($case.existing){
   $saved=@($observedTask.tasks|Where-Object name -eq $taskName);Assert ($saved.Count-eq1) 'Retry task snapshot missing'
   Assert ($saved[0].running-eq$case.running-and$saved[0].xml-ceq$oldXml-and$saved[0].security-ceq$oldSecurity) 'Retry prepared altered task intent/definition/security'
   Assert ((Task).Enabled-eq$case.enabled-and(((CountInstances)-gt0)-eq$case.running)) 'Successful retry changed runtime/login intent'
   $savedKey=@($observed.registry|Where-Object path -like '*Uninstall\Ottrin')[0]
   $binary=@($savedKey.values|Where-Object name -eq 'NoModify')[0];$qword=@($savedKey.values|Where-Object name -eq 'NoRepair')[0]
   Assert ($binary.kind-eq3-and(@($binary.value)-join ',')-ceq'0,255'-and$qword.kind-eq11-and$qword.value-eq4294967297) 'Retry snapshot lost typed registry values'
  }else{$absent=@($observedTask.tasks);Assert ($absent.Count-eq1-and$absent[0].name-ceq$taskName-and$null-eq$absent[0].xml-and$null-eq$absent[0].security-and$absent[0].running-eq$false) 'Fresh retry did not capture the exact absent task record';$null=Live}
  VerifyPayload;CheckData
  if($case.existing){Assert ([OttrinInstallIdentity]::Read($db)-ceq$dbId) 'Successful retry replaced saved index'}
  $observed|ConvertTo-Json -Depth 14|Set-Content "$r\observed-$name.json";$observedTask|ConvertTo-Json -Depth 8|Set-Content "$r\task-$name.json"
  $result.cases+=@{name=$name;point=$case.point;existing=$case.existing;enabled=$case.enabled;running=$case.running;status='pass';early_daemon_untouched=$true;old_root=$oldId;new_transaction_root=$observed.root;payload_files=@($manifest.PSObject.Properties).Count;preferences_and_source_preserved=$true;existing_saved_index_identity_preserved=$case.existing;task_intent_xml_security_verified=$true;typed_registration_snapshot_verified=$case.existing}
  Phase ('uninstall-'+$name);$null=Run $controller '--stop';Copy-Item "$install\Uninstall.exe" "$r\uninstall.exe" -Force;$null=Run "$r\uninstall.exe" "/S _?=$install"
  Assert (-not(Test-Path $install)-and(TaskAbsent)) 'Case uninstall left payload/task';Assert ((Get-FileHash $pref).Hash-ceq$prefHash) 'Uninstall changed preferences'
 }
 $result.current_installer_sha256=(Get-FileHash "$b\current.exe").Hash.ToLowerInvariant();$result.status='pass';Phase 'accepted'
}catch{$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally{
 $env:OTTRIN_QA_PAUSE_AT=$null;$cleanupErrors=@()
 foreach($job in $active){try{StopTree $job}catch{$cleanupErrors+=$_.Exception.Message};$job.process.Dispose()}
 if($result.status-ne'pass'){
  try{$null=Run $controller '--stop'}catch{$cleanupErrors+=$_.Exception.Message}
  if($null-ne$folder){try{$t=Task;$t.Enabled=$false;$t.Stop(0)}catch{if($_.Exception.GetBaseException().HResult-ne-2147024894){$cleanupErrors+=$_.Exception.Message}}}
 }
 if($cleanupErrors.Count){$result.status='fail';$result.cleanup_errors=$cleanupErrors}
 $result|ConvertTo-Json -Depth 14|Set-Content "$r\RESULTS.json"
}
if($result.status-ne'pass'){exit 1}
