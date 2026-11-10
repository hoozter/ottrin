param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExpectedSid)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$out=Join-Path $Root 'results'
$result=@{status='running';scope='Exact ordinary-user NSIS installed picker timing in default and forced OpenGL modes';picker_cases=@()};$finalStatus='failed native folder picker'
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
 $timer=[Diagnostics.Stopwatch]::StartNew()
 $process=[Diagnostics.Process]::Start($start)
 $entry=@{case=$case;title="Ottrin — $title";process=$process;timer=$timer;start_ms=$timer.Elapsed.TotalMilliseconds;renderer_arguments=$rendererArguments}
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
  Require (-not$entry.process.HasExited) ('Picker exited before mapping: '+$entry.case)
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
 return @{case=$entry.case;window_mapped_upper_bound_ms=$entry.mapped_ms;normal_cancel=$true;response=$response;start_ms=$entry.start_ms;request_sent_ms=$entry.request_sent_ms;renderer_arguments=$entry.renderer_arguments}
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
 Phase 'install exact current NSIS'
 $install=Join-Path $env:LOCALAPPDATA 'Programs\Ottrin'
 Require (-not(Test-Path $install)) 'Existing installation must not be touched'
 $ownsInstall=$true
 $installer=Start-Process (Join-Path $Root 'current-setup.exe') -ArgumentList '/S' -PassThru -Wait
 Require ($installer.ExitCode-eq0) 'Current NSIS install failed'
 $app=Join-Path $install 'bin\ottrin.exe'
 $daemon=Start-Process (Join-Path $install 'bin\ottrin-indexd.exe') -ArgumentList '--stop' -PassThru -Wait
 Require ($daemon.ExitCode-eq0) 'Could not stop installed index'
 Phase 'installed picker first and repeat launch'
 $binaryHash=(Get-FileHash $app).Hash.ToLowerInvariant()
 Require ($binaryHash-eq$manifest.'bin/ottrin.exe') 'Installed binary mismatch'
 $result.binary_sha256=$binaryHash
 Require (@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue).Count-eq0) 'Unexpected initial daemon'
 $config.theme='Dark'
 [IO.File]::WriteAllText($pref,($config|ConvertTo-Json -Depth 60),[Text.UTF8Encoding]::new($false))
 $before=(Get-FileHash $pref).Hash
 foreach($trial in @(@('default-first',''),@('default-repeat',''),@('opengl-first','--renderer opengl'),@('opengl-repeat','--renderer opengl'))){
  $entry=LaunchPicker $app $trial[0] $trial[1]
  WaitMapped $entry
  $result.picker_cases+=(CancelPicker $entry)
  Save
 }
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
