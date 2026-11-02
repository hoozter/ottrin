param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExpectedSid)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$out=Join-Path $Root 'results'
$result=@{status='running';scope='Exact ordinary-user portable chooser and NSIS fresh/upgrade/intent/refusal/retry workflows';picker_cases=@();runtime_cases=@()};$finalStatus='failed native NSIS fixture';$lock=$null;$active=$null;$app=$null
function Require($value,$message){if(-not$value){throw $message}}
function Save {$temporary=Join-Path $out 'RESULT.next';[IO.File]::WriteAllText($temporary,($result|ConvertTo-Json -Depth 25),[Text.UTF8Encoding]::new($false));$destination=Join-Path $out 'RESULT.json';if(Test-Path $destination){[IO.File]::Replace($temporary,$destination,[NullString]::Value)}else{[IO.File]::Move($temporary,$destination)}}
function Phase($name){$result.phase=$name;Save}
function Run([string]$program,[string]$arguments,[string]$name){
 $start=[Diagnostics.ProcessStartInfo]::new($program,$arguments);$start.UseShellExecute=$false;$start.RedirectStandardOutput=$true;$start.RedirectStandardError=$true;$start.CreateNoWindow=$true
 $process=[Diagnostics.Process]::Start($start)
 try{$stdout=$process.StandardOutput.ReadToEndAsync();$stderr=$process.StandardError.ReadToEndAsync();Require ($process.WaitForExit(180000)) ('Owned child timed out: '+$name);Require ($stdout.Wait(5000)-and$stderr.Wait(5000)) ('Owned output remained open: '+$name);$value=@{exit=$process.ExitCode;stdout=$stdout.Result;stderr=$stderr.Result};[IO.File]::WriteAllText((Join-Path $out ($name+'-process.json')),($value|ConvertTo-Json -Depth 5),[Text.UTF8Encoding]::new($false));return $value}
 finally{if(-not$process.HasExited){& "$env:SystemRoot\System32\taskkill.exe" /PID $process.Id /T /F|Out-Null;$null=$process.WaitForExit(5000)};$process.Dispose()}
}
function Success($program,$arguments,$name){$value=Run $program $arguments $name;Require ($value.exit-eq0) ('Native child failed: '+$name+' exit '+$value.exit+' '+$value.stderr);return $value}
function InstalledTask {
 $ownedTasks=@($folder.GetTasks(1)|Where-Object {$_.Name.StartsWith('Ottrin Index')-and@($_.Definition.Actions|Where-Object {([string]$_.Path).Trim('"')-ieq$daemon}).Count-eq1})
 Require ($ownedTasks.Count-eq1) 'Expected one exact installed-daemon task';$principal=$ownedTasks[0].Definition.Principal.UserId;if(-not$principal.StartsWith('S-')){$principal=([Security.Principal.NTAccount]::new($principal)).Translate([Security.Principal.SecurityIdentifier]).Value};Require ($principal-eq$ExpectedSid) 'Installed task belongs to another account';return $ownedTasks[0]
}
function Live {$task=InstalledTask;$instances=$task.GetInstances(0);$processes=@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue|Where-Object {$_.Path-ieq$daemon});Require ($instances.Count-eq1-and$processes.Count-eq1) 'Expected one actual task instance and daemon';return @{pid=$processes[0].Id;instance=$instances.Item(1).InstanceGuid}}
function FileId([string]$path){$file=[IO.File]::Open($path,'Open','Read',([IO.FileShare]::ReadWrite-bor[IO.FileShare]::Delete));try{$info=New-Object OttrinOwnedFileId+Info;Require ([OttrinOwnedFileId]::GetFileInformationByHandle($file.SafeFileHandle.DangerousGetHandle(),[ref]$info)) 'Native file identity failed';return $info.volume.ToString('X8')+':'+$info.indexHigh.ToString('X8')+$info.indexLow.ToString('X8')}finally{$file.Dispose()}}
function VerifyPayload($manifest){
 foreach($entry in $manifest.PSObject.Properties){Require ((Get-FileHash (Join-Path $install $entry.Name)).Hash.ToLowerInvariant()-eq$entry.Value) ('Installed payload mismatch: '+$entry.Name)}
 Require (@(Get-ChildItem $install -Recurse -File).Count-eq(@($manifest.PSObject.Properties).Count+1)) 'Expected exact payload plus generated uninstaller'
 Require (-not(Test-Path ($install+'.ottrin-setup'))-and-not(Test-Path ($install+'.ottrin-completed'))) 'Completed setup transaction residue'
}
function VerifyData {
 Require ((Get-FileHash $pref).Hash-eq$prefHash) 'Saved preferences changed';Require ((Get-FileHash $source).Hash-eq$sourceHash) 'Source file changed'
 $query=(Success $app 'search upgrade-proof --json' ('query-'+$result.phase)).stdout|ConvertFrom-Json;Require ($query.total-eq1) 'Saved index query missing'
}
function Ready {
 $deadline=[DateTime]::UtcNow.AddSeconds(60)
 do{$state=(Success $app 'index status --json' ('status-'+$result.phase)).stdout|ConvertFrom-Json;if($state.status-eq'ready'-and$state.watcher_ready-and$state.watched_roots-eq1){Require ($state.indexed_items-eq1) 'Index escaped its one-file fixture';return};Start-Sleep -Milliseconds 500}while([DateTime]::UtcNow-lt$deadline)
 throw 'Installed daemon did not become ready'
}
function MainWindow($name){
 $process=Start-Process $app -ArgumentList @(('"'+$indexRoot+'"'),'--renderer','opengl') -PassThru
 try{
  $deadline=[DateTime]::UtcNow.AddSeconds(45);$window=[IntPtr]::Zero
  do{Require (-not$process.HasExited) 'Main window exited before mapping';$window=[PickerNative]::Window($process.Id,'Ottrin');if($window-ne[IntPtr]::Zero){break};Start-Sleep -Milliseconds 200}while([DateTime]::UtcNow-lt$deadline)
  Require ($window-ne[IntPtr]::Zero) 'Installed main window did not map';[PickerNative]::Place($window);[PickerNative]::Focus($window);Start-Sleep -Milliseconds 700
  [PickerNative]::Shot($window,(Join-Path $out ($name+'.png')));$geometry=[PickerNative]::Geometry($window)
  Require ([OttrinCloseQA]::PostMessageW($window,0x10,[IntPtr]::Zero,[IntPtr]::Zero)) 'Normal main-window Close failed';Require ($process.WaitForExit(15000)) 'Main window did not close';$process.WaitForExit();Require ($process.ExitCode-eq0) 'Main window exited abnormally'
  $saved=Get-Content $pref -Raw|ConvertFrom-Json;Require ($saved.default_view_mode-eq'Miller') 'New binary lost saved Miller view';$script:prefHash=(Get-FileHash $pref).Hash
  return @{status='pass';exit=0;saved_view=$saved.default_view_mode;binary_sha256=(Get-FileHash $app).Hash.ToLowerInvariant();geometry=$geometry}
 }finally{if(-not$process.HasExited){$process.Kill();$null=$process.WaitForExit(5000)};$process.Dispose()}
}
function Uninstall($name){$runner=Join-Path $out 'owned-uninstall-runner.exe';Copy-Item (Join-Path $install 'Uninstall.exe') $runner -Force;return Run $runner ('/S _?='+$install) $name}
function NoInstallation {
 Require (-not(Test-Path $install)) 'Uninstall left the installation';Require (-not(Test-Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Ottrin')) 'Uninstall registration remains'
 Require (@($folder.GetTasks(1)|Where-Object {$_.Name.StartsWith('Ottrin Index')}).Count-eq0) 'Owned user index task remains'
 Require (@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue|Where-Object {$_.Path-ieq$daemon}).Count-eq0) 'Installed daemon remains';Require ((Get-FileHash $pref).Hash-eq$prefHash) 'Removal changed saved preferences';Require ((Get-FileHash $source).Hash-eq$sourceHash) 'Removal changed source'
}
function NativePath($path){return @{encoding='windows-utf16-le';units_hex=(-join $path.ToCharArray().ForEach({([int]$_).ToString('x4')}))}}
function Picker($binary,$theme,$direction,$shift,$baseline){
 $name=('picker-'+$theme.ToLowerInvariant()+'-'+$(if($baseline){'baseline'}else{$direction.ToLowerInvariant()+'-'+$(if($shift){'range'}else{'single'})}))
 $request=@{mode='open';multiple=$true;title='Selection proof';initial_directory=(NativePath $selection)}
 $start=[Diagnostics.ProcessStartInfo]::new($binary,'picker --renderer opengl');$start.UseShellExecute=$false;$start.RedirectStandardInput=$true;$start.RedirectStandardOutput=$true;$start.RedirectStandardError=$true;$process=[Diagnostics.Process]::Start($start)
 try{
  $stdout=$process.StandardOutput.ReadToEndAsync();$stderr=$process.StandardError.ReadToEndAsync();$process.StandardInput.Write(($request|ConvertTo-Json -Depth 5 -Compress));$process.StandardInput.Close()
  $deadline=[DateTime]::UtcNow.AddSeconds(30);$window=[IntPtr]::Zero
  do{Require (-not$process.HasExited) ('Chooser exited before mapping: '+$name);$window=[PickerNative]::Window($process.Id,'Ottrin — Selection proof');if($window-ne[IntPtr]::Zero){break};Start-Sleep -Milliseconds 100}while([DateTime]::UtcNow-lt$deadline)
  if($window-eq[IntPtr]::Zero){$unmatched=[PickerNative]::AnyWindow($process.Id);if($unmatched-ne[IntPtr]::Zero){$result.unmatched_picker_title=[PickerNative]::Title($unmatched);[PickerNative]::Shot($unmatched,(Join-Path $out ($name+'-unmatched.png')))}}
  Require ($window-ne[IntPtr]::Zero) 'Chooser did not map with its source-declared native title';[PickerNative]::Place($window);[PickerNative]::Focus($window);Start-Sleep -Milliseconds 900
  [PickerNative]::Shot($window,(Join-Path $out ($name+'-before.png')))
  [PickerNative]::Click($window,300,300);[System.Windows.Forms.SendKeys]::SendWait('{DOWN}');Start-Sleep -Milliseconds 180;[System.Windows.Forms.SendKeys]::SendWait('{DOWN}');Start-Sleep -Milliseconds 180
  if(-not$baseline){[System.Windows.Forms.SendKeys]::SendWait(($(if($shift){'+'}else{''})+'{'+$direction.ToUpperInvariant()+'}'));Start-Sleep -Milliseconds 180}
  [PickerNative]::Shot($window,(Join-Path $out ($name+'-selected.png')));[System.Windows.Forms.SendKeys]::SendWait('{ENTER}')
  Require ($process.WaitForExit(15000)) 'Chooser did not accept selection';$process.WaitForExit();Require ($stdout.Wait(5000)-and$stderr.Wait(5000)) 'Chooser output remained open';Require ($process.ExitCode-eq0) ('Chooser exit failed: '+$stderr.Result)
  $response=$stdout.Result|ConvertFrom-Json;Require $response.accepted ('Chooser did not accept: '+$name)
  $expectedNames=@('beta.txt');if(-not$baseline){if($direction-eq'Up'){$expectedNames=@('alpha.txt')}else{$expectedNames=@('gamma.txt')}};if($shift-and-not$baseline){$expectedNames+=@('beta.txt')}
  $expected=@($expectedNames|ForEach-Object {(NativePath (Join-Path $selection $_)).units_hex}|Sort-Object)
  $actual=@($response.paths|ForEach-Object {Require ($_.encoding-eq'windows-utf16-le') 'Chooser returned wrong native encoding';$hex=$_.units_hex;if($hex.StartsWith('005c005c003f005c')){$hex.Substring(16)}else{$hex}}|Sort-Object)
  Require (($actual-join'|')-eq($expected-join'|')) ('Wrong native selection: '+$name+' '+$stdout.Result)
  return @{case=$name;status='pass';exit=0;response=$response;expected_names=$expectedNames;binary_sha256=(Get-FileHash $binary).Hash.ToLowerInvariant()}
 }finally{if(-not$process.HasExited){$process.Kill();$null=$process.WaitForExit(5000)};$process.Dispose()}
}
try{
 $identity=[Security.Principal.WindowsIdentity]::GetCurrent();Require ($identity.User.Value-eq$ExpectedSid) 'Wrong actor SID';Require (-not([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Ordinary token required'
 $session=(Get-Process -Id $PID).SessionId;Require ($session-gt0) 'Actual interactive session required';$profilePath=[Environment]::GetFolderPath('UserProfile');[Environment]::SetEnvironmentVariable('HOME',$profilePath,'Process');$result.identity=@{sid=$ExpectedSid;profile=$profilePath;session=$session;administrator=$false}
 Phase 'waiting for observed owned desktop';$deadline=[DateTime]::UtcNow.AddMinutes(8);$readyPath=Join-Path $out 'DESKTOP_READY.json'
 while(-not(Test-Path $readyPath)){Require ([DateTime]::UtcNow-lt$deadline) 'Owned desktop was not observed';Start-Sleep -Milliseconds 250}
 $ready=Get-Content $readyPath -Raw|ConvertFrom-Json;$owner=Get-Content 'C:\OttrinMsiCleanClientResults\OWNER.json' -Raw|ConvertFrom-Json;Require ($ready.sid-eq$ExpectedSid-and$ready.nonce-eq$owner.nonce) 'Wrong desktop signal';$result.desktop_ready=$true
 & ([ScriptBlock]::Create([IO.File]::ReadAllText((Join-Path $Root 'native-controls.ps1'))));[PickerNative]::Init()
 Add-Type @'
using System;using System.Runtime.InteropServices;
public static class OttrinCloseQA {[DllImport("user32.dll")]public static extern bool PostMessageW(IntPtr h,uint message,IntPtr w,IntPtr l);}
public static class OttrinOwnedFileId {
 [StructLayout(LayoutKind.Sequential)]public struct Info {public uint attr,creationLo,creationHi,accessLo,accessHi,writeLo,writeHi,volume,sizeHigh,sizeLow,links,indexHigh,indexLow;}
 [DllImport("kernel32.dll",SetLastError=true)]public static extern bool GetFileInformationByHandle(IntPtr handle,out Info info);
}
'@
 $packageInput=Get-Content (Join-Path $Root 'application-input.json') -Raw|ConvertFrom-Json;foreach($file in $packageInput.files){Require ((Get-FileHash (Join-Path $Root $file.name)).Hash.ToLowerInvariant()-eq$file.sha256) 'Package input mismatch'};$result.package=$packageInput
 $install=Join-Path $env:LOCALAPPDATA 'Programs\Ottrin';$app=Join-Path $install 'bin\ottrin.exe';$daemon=Join-Path $install 'bin\ottrin-indexd.exe';Require (-not(Test-Path $install)) 'Existing install must not be touched'
 $manifest=Get-Content (Join-Path $Root 'payload.json') -Raw|ConvertFrom-Json;$oldManifest=Get-Content (Join-Path $Root 'previous-payload.json') -Raw|ConvertFrom-Json
 $indexRoot=Join-Path $profilePath 'OttrinOwnedIndex';$selection=Join-Path $profilePath 'OttrinOwnedSelection';Require (-not(Test-Path $indexRoot)-and-not(Test-Path $selection)) 'Existing fixture must not be touched';$null=New-Item -ItemType Directory $indexRoot,$selection
 $source=Join-Path $indexRoot 'upgrade-proof.txt';[IO.File]::WriteAllText($source,'Native ordinary installer source remains unchanged.');$sourceHash=(Get-FileHash $source).Hash
 foreach($name in @('alpha.txt','beta.txt','gamma.txt')){[IO.File]::WriteAllText((Join-Path $selection $name),$name)}
 $config=Get-Content (Join-Path $Root 'config-template.json') -Raw|ConvertFrom-Json;$config.bookmarks=@();$config.theme='Dark';$config.default_view_mode='Miller';$config.installed_office_previews_enabled=$false;$config.search.include_roots=@($indexRoot);$config.search.exclude_roots=@();$config.search.content_indexing_enabled=$false
 $pref=Join-Path $env:APPDATA 'ottrin\config.json';Require (-not(Test-Path $pref)) 'Existing preferences must not be replaced';$null=[IO.Directory]::CreateDirectory((Split-Path $pref));$configBytes=($config|ConvertTo-Json -Depth 60);[IO.File]::WriteAllText($pref,$configBytes,[Text.UTF8Encoding]::new($false));$prefHash=(Get-FileHash $pref).Hash
 $scheduler=New-Object -ComObject Schedule.Service;$scheduler.Connect();$folder=$scheduler.GetFolder('\')
 Phase 'portable chooser without indexing';Expand-Archive (Join-Path $Root 'portable.zip') (Join-Path $out 'portable');$portable=Join-Path $out 'portable\Ottrin-0.1.0-windows-x64\bin\ottrin.exe';Require ((Get-FileHash $portable).Hash.ToLowerInvariant()-eq$manifest.'bin/ottrin.exe') 'Portable binary mismatch'
 Require (@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue).Count-eq0) 'Unexpected initial daemon'
 foreach($theme in @('Dark','Light')){
  $config.theme=$theme;[IO.File]::WriteAllText($pref,($config|ConvertTo-Json -Depth 60),[Text.UTF8Encoding]::new($false));$before=(Get-FileHash $pref).Hash
  $result.picker_cases+=Picker $portable $theme 'Down' $false $true
  foreach($direction in @('Up','Down')){foreach($shift in @($false,$true)){$result.picker_cases+=Picker $portable $theme $direction $shift $false;Save}}
  Require ((Get-FileHash $pref).Hash-eq$before) 'Chooser changed main preferences'
 }
 Require (@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue).Count-eq0) 'Chooser started indexing';foreach($file in Get-ChildItem $selection -File){Require ([IO.File]::ReadAllText($file.FullName)-eq$file.Name) 'Chooser modified source'};$result.portable_picker_without_index='pass'
 [IO.File]::WriteAllText($pref,$configBytes,[Text.UTF8Encoding]::new($false));$prefHash=(Get-FileHash $pref).Hash
 Phase 'current fresh install';$result.fresh_install_exit=(Success (Join-Path $Root 'current-setup.exe') '/S' 'fresh-install').exit;VerifyPayload $manifest;$null=Success $app 'index update --json' 'fresh-index';Ready;VerifyData;$result.fresh_main=MainWindow 'fresh-main';$prefHash=(Get-FileHash $pref).Hash
 Phase 'current fresh uninstall';$removed=Uninstall 'fresh-uninstall';Require ($removed.exit-eq0) 'Fresh current uninstall failed';NoInstallation;$result.fresh_install_uninstall='pass'
 Phase 'accepted baseline install';$null=Success (Join-Path $Root 'previous-setup.exe') '/S' 'baseline-install';VerifyPayload $oldManifest;$null=Success $app 'index update --json' 'baseline-index';Ready;$result.previous_main=MainWindow 'previous-main';$prefHash=(Get-FileHash $pref).Hash;VerifyData
 $before=Live;$task=InstalledTask;$security=$task.GetSecurityDescriptor(7);$enabled=$task.Enabled;$database=Join-Path $env:LOCALAPPDATA 'ottrin\search.db';$databaseId=FileId $database;$result.before_upgrade=@{live=$before;database_identity=$databaseId;enabled=$enabled;security=$security;preferences=$prefHash}
 Phase 'current upgrade with running index';$null=Success (Join-Path $Root 'current-setup.exe') '/S' 'running-upgrade';VerifyPayload $manifest;Ready;VerifyData;$after=Live;Require ($before.pid-ne$after.pid) 'Changed daemon was not replaced';Require ((InstalledTask).Enabled-eq$enabled-and(InstalledTask).GetSecurityDescriptor(7)-eq$security) 'Upgrade changed task intent or security';Require ((FileId $database)-eq$databaseId) 'Upgrade replaced native database identity';$result.current_main=MainWindow 'upgraded-main';Require ((Get-FileHash $pref).Hash-eq$result.before_upgrade.preferences) 'Updated GUI changed saved configuration';$result.after_upgrade=@{live=$after;database_identity=(FileId $database);preferences=(Get-FileHash $pref).Hash};$result.running_index_upgrade='pass'
 foreach($case in @(@{enabled=$false;running=$true},@{enabled=$true;running=$false},@{enabled=$false;running=$false})){
  Phase ('reinstall-'+$case.enabled+'-'+$case.running);$task=InstalledTask;$task.Enabled=$case.enabled;if(-not$case.running){$null=Success $daemon '--stop' ('stop-'+$result.phase)}
  $null=Success (Join-Path $Root 'current-setup.exe') '/S' ('setup-'+$result.phase);VerifyPayload $manifest;Require ((InstalledTask).Enabled-eq$case.enabled) 'Reinstall changed login intent';Require (((InstalledTask).GetInstances(0).Count-gt0)-eq$case.running) 'Reinstall changed running intent';Require ((FileId $database)-eq$databaseId) 'Reinstall replaced saved database';Require ((Get-FileHash $pref).Hash-eq$prefHash) 'Reinstall changed preferences';$result.runtime_cases+=@{enabled=$case.enabled;running=$case.running;status='pass'}
 }
 $task=InstalledTask;$task.Enabled=$true;$null=$task.Run($null);Ready;VerifyData
 Phase 'real locked-cache uninstall refusal';$liveBefore=Live;$taskBefore=(InstalledTask).Xml;$securityBefore=(InstalledTask).GetSecurityDescriptor(7)
 $cache=Join-Path $env:LOCALAPPDATA 'ottrin\preview_cache';$null=[IO.Directory]::CreateDirectory($cache);$held=Join-Path $cache 'owned-delete-denial';[IO.File]::WriteAllText($held,'Owned fixture lock');$lock=[IO.File]::Open($held,'Open','Read','Read');$refused=Uninstall 'locked-uninstall';Require ($refused.exit-ne0) 'Locked-cache uninstall unexpectedly succeeded';VerifyPayload $manifest;VerifyData;$liveAfter=Live
 Require ($liveBefore.pid-eq$liveAfter.pid-and$liveBefore.instance-eq$liveAfter.instance) 'Refused uninstall disturbed live index';Require ((InstalledTask).Xml-eq$taskBefore-and(InstalledTask).GetSecurityDescriptor(7)-eq$securityBefore) 'Refused uninstall changed task';Require ((FileId $database)-eq$databaseId) 'Refused uninstall replaced index';$result.refused_uninstall=@{status='pass';exit=$refused.exit;before=$liveBefore;after=$liveAfter;task_and_security_preserved=$true;database_identity=(FileId $database);payload_preserved=$true;preferences_preserved=$true;source_preserved=$true}
 $lock.Dispose();$lock=$null;Phase 'supported uninstall retry';$retry=Uninstall 'uninstall-retry';Require ($retry.exit-eq0) 'Supported uninstall retry failed';NoInstallation;$result.uninstall_retry='pass';$result.payload_files=@($manifest.PSObject.Properties).Count;$finalStatus='pass'
}catch{$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace;$finalStatus='failed native NSIS fixture'}
finally{
 if($lock){$lock.Dispose()}
 if($app-and(Test-Path $app)){try{$cleanup=Run $app 'index service-remove' 'fixture-service-cleanup';$result.service_cleanup_exit=$cleanup.exit;if($cleanup.exit-ne0){$finalStatus='failed owned service teardown'}}catch{$result.cleanup_error=$_.Exception.Message;$finalStatus='failed owned service teardown'}}
 $portableRoot=Join-Path $out 'portable';if(Test-Path $portableRoot){Remove-Item $portableRoot -Recurse -Force};Remove-Item (Join-Path $out 'owned-uninstall-runner.exe') -Force -ErrorAction SilentlyContinue
 $result.owned_portable_extraction_removed=(-not(Test-Path $portableRoot));$result.status=$finalStatus;Save
}
