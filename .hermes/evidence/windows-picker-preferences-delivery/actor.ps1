param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExpectedSid)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$out=Join-Path $Root 'results'
$result=@{status='running';scope='Overlapping exact portable and installed pickers with independent preference changes';picker_cases=@()};$finalStatus='failed native folder picker'
function Require($value,$message){if(-not$value){throw $message}}
function Save {$temporary=Join-Path $out 'RESULT.next';[IO.File]::WriteAllText($temporary,($result|ConvertTo-Json -Depth 25),[Text.UTF8Encoding]::new($false));$destination=Join-Path $out 'RESULT.json';if(Test-Path $destination){[IO.File]::Replace($temporary,$destination,[NullString]::Value)}else{[IO.File]::Move($temporary,$destination)}}
function Phase($name){$result.phase=$name;Save}
function NativePath($path){return @{encoding='windows-utf16-le';units_hex=(-join $path.ToCharArray().ForEach({([int]$_).ToString('x4')}))}}

$install=$null
$ownsInstall=$false
$pickers=[Collections.Generic.List[object]]::new()
function LaunchPicker($binary,$case,$rendererArguments){
 $title="Resource check $case"
 $request=@{mode='open';title=$title;initial_directory=(NativePath $selection)}
 $start=[Diagnostics.ProcessStartInfo]::new($binary,('picker '+$rendererArguments))
 $start.UseShellExecute=$false
 $start.RedirectStandardInput=$true
 $start.RedirectStandardOutput=$true
 $start.RedirectStandardError=$true
 $start.EnvironmentVariables['OTTRIN_TRACE_PICKER_STARTUP']='1'
 $launchEpoch=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
 $timer=[Diagnostics.Stopwatch]::StartNew()
 $process=[Diagnostics.Process]::Start($start)
 $entry=@{case=$case;title="Ottrin — $title";process=$process;timer=$timer;start_ms=$timer.Elapsed.TotalMilliseconds;launch_unix_ms=$launchEpoch;renderer_arguments=$rendererArguments}
 $pickers.Add($entry)
 $entry.stdout=$process.StandardOutput.ReadToEndAsync()
 $entry.stderr=$process.StandardError.ReadToEndAsync()
 $process.StandardInput.Write(($request|ConvertTo-Json -Depth 5 -Compress))
 $process.StandardInput.Close()
 $entry.request_sent_ms=$timer.Elapsed.TotalMilliseconds
 return $entry
}
function WaitMapped($entry){
 $deadline=[DateTime]::UtcNow.AddSeconds(25)
 $window=[IntPtr]::Zero
 do{
  if($entry.process.HasExited){
   $entry.process.WaitForExit()
   $result.failed_launch=@{case=$entry.case;exit=$entry.process.ExitCode;stderr=$entry.stderr.Result;stdout=$entry.stdout.Result;start_ms=$entry.start_ms}
   throw ('Picker exited before mapping: '+$entry.case)
  }
  $window=[PickerNative]::Window($entry.process.Id,$entry.title)
  if($window-ne[IntPtr]::Zero){break}
  Start-Sleep -Milliseconds 60
 }while([DateTime]::UtcNow-lt$deadline)
 Require ($window-ne[IntPtr]::Zero) ('Missing owned picker window: '+$entry.case)
 $entry.window=$window
 $entry.mapped_ms=[Math]::Round($entry.timer.Elapsed.TotalMilliseconds,2)
}
function CancelPicker($entry){
 Require ([PickerNative]::Window($entry.process.Id,$entry.title)-eq$entry.window) 'Owned picker window changed'
 [PickerNative]::Focus($entry.window)
 [System.Windows.Forms.SendKeys]::SendWait('{ESC}')
 Require ($entry.process.WaitForExit(12000)) ('Escape did not close '+$entry.case)
 $entry.process.WaitForExit()
 Require ($entry.process.ExitCode-eq0-and$entry.stdout.Wait(5000)-and$entry.stderr.Wait(5000)) ('Picker process failed: '+$entry.case)
 $response=$entry.stdout.Result|ConvertFrom-Json
 Require ($response.accepted-eq$false-and@($response.paths).Count-eq0) ('Picker did not return cancellation: '+$entry.case)
 $trace=@([regex]::Matches($entry.stderr.Result,'(?m)^ottrin picker startup: (\w+) entry_unix_ms=(\d+) elapsed_ms=([\d.]+)\r?$') | ForEach-Object {
  @{phase=$_.Groups[1].Value;entry_unix_ms=[long]$_.Groups[2].Value;elapsed_ms=[double]::Parse($_.Groups[3].Value,[Globalization.CultureInfo]::InvariantCulture)}
 })
 Require ($trace.Count-eq6) ('Missing startup trace for '+$entry.case+': '+$entry.stderr.Result)
 return @{case=$entry.case;process_id=$entry.process.Id;window_mapped_upper_bound_ms=$entry.mapped_ms;normal_cancel=$true;response=$response;trace=$trace;launch_unix_ms=$entry.launch_unix_ms;start_ms=$entry.start_ms;request_sent_ms=$entry.request_sent_ms;renderer_arguments=$entry.renderer_arguments}
}
function InvokePickerButton($entry,$name){
 Require ([PickerNative]::Window($entry.process.Id,$entry.title)-eq$entry.window) 'Owned picker window changed'
 [PickerNative]::Focus($entry.window)
 $tree=[Windows.Automation.AutomationElement]::FromHandle($entry.window)
 $condition=[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,$name)
 $deadline=[DateTime]::UtcNow.AddSeconds(20)
 do{
  $matches=@($tree.FindAll([Windows.Automation.TreeScope]::Descendants,$condition)|Where-Object {$_.Current.ControlType-eq[Windows.Automation.ControlType]::Button-and-not$_.Current.IsOffscreen})
  if($matches.Count-eq1){
   Require $matches[0].Current.IsEnabled ('Picker button disabled: '+$name)
   $matches[0].GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
   Start-Sleep -Milliseconds 200
   return
  }
  Start-Sleep -Milliseconds 100
 }while([DateTime]::UtcNow-lt$deadline)
 throw ('Missing unique picker button: '+$name)
}
try{
 $identity=[Security.Principal.WindowsIdentity]::GetCurrent();Require ($identity.User.Value-eq$ExpectedSid) 'Wrong actor SID';Require (-not([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Ordinary token required'
 $session=(Get-Process -Id $PID).SessionId;Require ($session-gt0) 'Actual interactive session required';$profilePath=[Environment]::GetFolderPath('UserProfile');[Environment]::SetEnvironmentVariable('HOME',$profilePath,'Process');$result.identity=@{sid=$ExpectedSid;profile=$profilePath;session=$session;administrator=$false}
 Phase 'waiting for observed owned desktop';$deadline=[DateTime]::UtcNow.AddMinutes(8);$readyPath=Join-Path $out 'DESKTOP_READY.json'
 while(-not(Test-Path $readyPath)){Require ([DateTime]::UtcNow-lt$deadline) 'Owned desktop was not observed';Start-Sleep -Milliseconds 250}
 $ready=Get-Content $readyPath -Raw|ConvertFrom-Json;$owner=Get-Content 'C:\OttrinMsiCleanClientResults\OWNER.json' -Raw|ConvertFrom-Json;Require ($ready.sid-eq$ExpectedSid-and$ready.nonce-eq$owner.nonce) 'Wrong desktop signal';$result.desktop_ready=$true
 & ([ScriptBlock]::Create([IO.File]::ReadAllText((Join-Path $Root 'native-controls.ps1'))));[PickerNative]::Init()
 $packageInput=Get-Content (Join-Path $Root 'application-input.json') -Raw|ConvertFrom-Json
 foreach($file in $packageInput.files){Require ((Get-FileHash (Join-Path $Root $file.name)).Hash.ToLowerInvariant()-eq$file.sha256) 'Package input mismatch'}
 $result.package=$packageInput
 $manifest=Get-Content (Join-Path $Root 'payload.json') -Raw|ConvertFrom-Json
 $selection=Join-Path $profilePath 'OttrinOwnedSelection';Require (-not(Test-Path $selection)) 'Existing fixture must not be touched'
 $null=New-Item -ItemType Directory $selection
 $file=Join-Path $selection 'between.txt';[IO.File]::WriteAllText($file,'fixture bytes')
 $fileHash=(Get-FileHash $file).Hash
 $config=Get-Content (Join-Path $Root 'config-template.json') -Raw|ConvertFrom-Json
 $config.bookmarks=@();$config.search.include_roots=@($selection);$config.search.exclude_roots=@();$config.search.content_indexing_enabled=$false
 $pref=Join-Path $env:APPDATA 'ottrin\config.json';Require (-not(Test-Path $pref)) 'Existing preferences must not be replaced'
 $null=[IO.Directory]::CreateDirectory((Split-Path $pref))
 Phase 'diagnostic portable picker startup phases'
 Expand-Archive (Join-Path $Root 'portable.zip') (Join-Path $out 'portable')
 $portable=Join-Path $out 'portable\Ottrin-0.1.0-windows-x64\bin\ottrin.exe'
 $binaryHash=(Get-FileHash $portable).Hash.ToLowerInvariant()
 Require ($binaryHash-eq$manifest.'bin/ottrin.exe') 'Portable binary mismatch'
 $result.binary_sha256=$binaryHash
 Require (@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue).Count-eq0) 'Unexpected initial daemon'
 $config.theme='Dark'
 [IO.File]::WriteAllText($pref,($config|ConvertTo-Json -Depth 60),[Text.UTF8Encoding]::new($false))
 $before=(Get-FileHash $pref).Hash
 Phase 'install exact NSIS'
 $install=Join-Path $env:LOCALAPPDATA 'Programs\Ottrin'
 Require (-not(Test-Path $install)) 'Existing installation must not be touched'
 $ownsInstall=$true
 $installer=Start-Process (Join-Path $Root 'current-setup.exe') -ArgumentList '/S' -PassThru -Wait
 Require ($installer.ExitCode-eq0) 'Current NSIS install failed'
 $app=Join-Path $install 'bin\ottrin.exe'
 $result.installed_binary_sha256=(Get-FileHash $app).Hash.ToLowerInvariant()
 Require ($result.installed_binary_sha256-eq$binaryHash) 'Installed binary differs from portable'
 $daemon=Start-Process (Join-Path $install 'bin\ottrin-indexd.exe') -ArgumentList '--stop' -PassThru -Wait
 Require ($daemon.ExitCode-eq0) 'Could not stop installed index'
 $before=(Get-FileHash $pref).Hash
 Phase 'overlapping portable and installed picker preferences'
 Add-Type -AssemblyName UIAutomationClient
 Add-Type -AssemblyName UIAutomationTypes
 $pickerPrefs=Join-Path (Split-Path $pref) 'picker.json'
 Require (-not(Test-Path $pickerPrefs)) 'Unexpected initial picker preferences'
 $stale=LaunchPicker $portable 'portable-untouched' '';WaitMapped $stale
 $grid=LaunchPicker $portable 'portable-grid' '';WaitMapped $grid
 $hidden=LaunchPicker $app 'installed-hidden' '';WaitMapped $hidden
 Require (@($stale.process.Id,$grid.process.Id,$hidden.process.Id|Select-Object -Unique).Count-eq3) 'Expected three picker processes'
 InvokePickerButton $grid 'Grid view'
 InvokePickerButton $hidden 'Show hidden files'
 $result.picker_cases+=(CancelPicker $grid)
 $prefs=Get-Content $pickerPrefs -Raw|ConvertFrom-Json
 Require ($prefs.grid-and-not$prefs.hidden) 'First picker did not persist grid'
 $result.picker_cases+=(CancelPicker $hidden)
 $prefs=Get-Content $pickerPrefs -Raw|ConvertFrom-Json
 Require ($prefs.grid-and$prefs.hidden) 'Second picker discarded the other change'
 $result.picker_cases+=(CancelPicker $stale)
 $prefs=Get-Content $pickerPrefs -Raw|ConvertFrom-Json
 Require ($prefs.grid-and$prefs.hidden) 'Untouched picker overwrote newer preferences'
 $result.grid_and_hidden_preserved=$true
 Require ((Get-FileHash $pref).Hash-eq$before) 'Picker changed main preferences'
 Require (@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue).Count-eq0) 'Chooser started indexing'
 Require ((Get-FileHash $file).Hash-eq$fileHash) 'Chooser modified the fixture file'
 $result.no_index=$true;$result.source_preserved=$true;$result.preferences_preserved=$true;$finalStatus='pass'
}catch{$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally{
 foreach($entry in $pickers){
  if(-not$entry.process.HasExited){$entry.process.Kill();$null=$entry.process.WaitForExit(5000)}
  $entry.process.Dispose()
 }
 $extraction=Join-Path $out 'portable'
 if(Test-Path $extraction){Remove-Item -LiteralPath $extraction -Recurse -Force}
 $result.owned_portable_extraction_removed=(-not(Test-Path $extraction))
 if($ownsInstall-and(Test-Path (Join-Path $install 'Uninstall.exe'))){
  $uninstall=Join-Path $out 'owned-uninstall.exe'
  Copy-Item (Join-Path $install 'Uninstall.exe') $uninstall
  $removed=Start-Process $uninstall -ArgumentList ('/S _?='+$install) -PassThru -Wait
  $result.uninstall_exit=$removed.ExitCode
  Remove-Item $uninstall
 }
 $result.owned_installation_removed=($ownsInstall-and-not(Test-Path $install))
 $result.status=$finalStatus
 Save
}
