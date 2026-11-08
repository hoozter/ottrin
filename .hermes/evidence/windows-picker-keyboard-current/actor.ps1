param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExpectedSid)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$out=Join-Path $Root 'results'
$report=@{status='running';scope='Exact NSIS install, native themed Save keyboard ownership and ordinary-user uninstall';cases=@()}
function Require($value,$message){if(-not$value){throw $message}}
function Save{
 $path=Join-Path $out 'RESULT.json';$temporary=$path+'.next'
 [IO.File]::WriteAllText($temporary,($report|ConvertTo-Json -Depth 20),[Text.UTF8Encoding]::new($false))
 if(Test-Path $path){[IO.File]::Replace($temporary,$path,[NullString]::Value)}else{[IO.File]::Move($temporary,$path)}
}
function Phase($name){$report.phase=$name;Save}
function Pump{[Windows.Forms.Application]::DoEvents();Start-Sleep -Milliseconds 50}
function WaitUntil($condition,$description){
 $deadline=[DateTime]::UtcNow.AddSeconds(30)
 do{if(& $condition){return};Pump}while([DateTime]::UtcNow-lt$deadline)
 throw ('Timed out: '+$description)
}
function Run($exe,$arguments,$name){
 $info=[Diagnostics.ProcessStartInfo]::new($exe,$arguments);$info.UseShellExecute=$false;$info.CreateNoWindow=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true
 $process=[Diagnostics.Process]::Start($info)
 try{
  $stdout=$process.StandardOutput.ReadToEndAsync();$stderr=$process.StandardError.ReadToEndAsync()
  Require ($process.WaitForExit(180000)) ('Owned child timed out: '+$name)
  Require ($stdout.Wait(5000)-and$stderr.Wait(5000)) 'Child streams did not finish'
  $value=@{exit=$process.ExitCode;stdout=$stdout.Result;stderr=$stderr.Result}
  [IO.File]::WriteAllText((Join-Path $out ($name+'.json')),($value|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
  Require ($value.exit-eq0) ('Native child failed: '+$name)
  return $value
 }finally{if(-not$process.HasExited){& "$env:SystemRoot\System32\taskkill.exe" /PID $process.Id /T /F|Out-Null;$null=$process.WaitForExit(10000)};$process.Dispose()}
}
& ([ScriptBlock]::Create([IO.File]::ReadAllText((Join-Path $Root 'native-controls.ps1'))))
[PickerNative]::Init();Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
function Tree($window){[Windows.Automation.AutomationElement]::FromHandle($window)}
function All($window){@((Tree $window).FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)|Where-Object {-not$_.Current.IsOffscreen})}
function Nodes($window,$name){@(All $window|Where-Object {$_.Current.Name-eq$name})}
function Node($window,$name,$kind){
 $script:found=$null
 WaitUntil { $candidates=@(Nodes $window $name|Where-Object {$_.Current.ControlType-eq$kind});if($candidates.Count-eq1){$script:found=$candidates[0];return $true};return $false } ('visible unique '+$name)
 return $script:found
}
function InvokeButton($window,$name){
 $script:actionButton=$null
 WaitUntil {
  $matches=@(Nodes $window $name|Where-Object {$_.Current.ControlType-eq[Windows.Automation.ControlType]::Button-and$_.Current.IsEnabled})
  if($matches.Count-eq1){$script:actionButton=$matches[0];return $true};return $false
 } ('enabled unique button '+$name)
 $script:actionButton.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke();Pump
}

function Key($window,$key){
 [PickerNative]::Focus($window)
 [Windows.Forms.SendKeys]::SendWait($key)
 Pump
}
function EditCount($window){
 @(Nodes $window 'Location'|Where-Object {$_.Current.ControlType-eq[Windows.Automation.ControlType]::Edit}).Count
}
function ReplaceCount($window){
 @(Nodes $window 'Replace'|Where-Object {$_.Current.ControlType-eq[Windows.Automation.ControlType]::Button-and$_.Current.IsEnabled}).Count
}
function Native([string]$value){
 return @{encoding='windows-utf16-le';units_hex=(-join($value.ToCharArray()|ForEach-Object {'{0:x4}' -f [int]$_}))}
}
function StopOwned($process){
 if(-not$process.HasExited){
  & "$env:SystemRoot\System32\taskkill.exe" /PID $process.Id /T /F | Out-Null
  Require ($process.WaitForExit(10000)) 'Owned picker did not stop'
 }
 $process.Dispose()
}
try{
 $identity=[Security.Principal.WindowsIdentity]::GetCurrent()
 Require ($identity.User.Value-eq$ExpectedSid) 'Unexpected actor identity'
 Require (-not([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Actor must remain ordinary'
 $ready=Join-Path $out 'DESKTOP_READY.json'
 Phase 'waiting for owned desktop'
 $deadline=[DateTime]::UtcNow.AddMinutes(8)
 while(-not(Test-Path $ready)){Require ([DateTime]::UtcNow-lt$deadline) 'No owned desktop readiness';Pump}
 Require ((Get-Content $ready -Raw|ConvertFrom-Json).sid-eq$ExpectedSid) 'Wrong desktop readiness owner'
 $packageInput=Get-Content (Join-Path $Root 'application-input.json') -Raw|ConvertFrom-Json
 $setup=Join-Path $Root 'current-setup.exe'
 Require ((Get-FileHash -LiteralPath $setup).Hash.ToLowerInvariant()-eq$packageInput.files[0].sha256) 'Installer bytes changed'
 $install=Join-Path $env:LOCALAPPDATA 'Programs\Ottrin'
 Require (-not(Test-Path $install)) 'Fixture requires a fresh install'
 Phase 'install'
 $null=Run $setup '/S' 'install'
 $payload=Get-Content (Join-Path $Root 'payload.json') -Raw|ConvertFrom-Json
 foreach($file in $payload.PSObject.Properties){
  $path=Join-Path $install $file.Name
  Require ((Get-FileHash -LiteralPath $path).Hash.ToLowerInvariant()-eq$file.Value) ('Installed payload differs: '+$file.Name)
 }
 $report.payloadFiles=@($payload.PSObject.Properties).Count
 Require ($report.payloadFiles-eq59) 'Expected all payload members'
 Require (@(Get-ChildItem -LiteralPath $install -Recurse -File).Count-eq60) 'Unexpected installed payload file count'
 $app=Join-Path $install 'bin\ottrin.exe'
 $corpus=Join-Path $env:USERPROFILE 'OttrinPickerCorpus';New-Item -ItemType Directory $corpus|Out-Null
 $name='existing '+[char]0x03a9+'.txt';$original=Join-Path $corpus $name
 [IO.File]::WriteAllText($original,'unchanged caller bytes')
 $originalHash=(Get-FileHash -LiteralPath $original).Hash
 $configDirectory=Join-Path $env:APPDATA 'ottrin';New-Item -ItemType Directory $configDirectory -Force|Out-Null
 $configFile=Join-Path $configDirectory 'config.json'
 $config=Get-Content (Join-Path $Root 'config-template.json') -Raw|ConvertFrom-Json
 $dark=@{window_border=@(234,57,118,255);window_border_width=4;app_radius=24;border_radius=2}
 $light=@{window_border=@(234,57,118,255);window_border_width=4;app_radius=0;border_radius=2;panel=@(231,237,244,255);panel_raised=@(207,218,231,255);text=@(28,35,45,255);text_heading=@(19,28,42,255);text_muted=@(50,60,76,255);button_bg=@(197,211,229,255);button_text=@(21,32,47,255)}
 $config.active_custom_theme='Picker confirmation QA'
 $config.custom_themes=@(@{name=$config.active_custom_theme;base_preset='Ottrin';dark=$dark;light=$light})
 foreach($variant in @('Dark','Light')){
  Phase ('installed picker '+$variant)
  $config.theme=$variant
  [IO.File]::WriteAllText($configFile,($config|ConvertTo-Json -Depth 40),[Text.UTF8Encoding]::new($false))
  $configHash=(Get-FileHash $configFile).Hash
  $title='Native Save confirmation '+$variant
  $nativeTitle='Ottrin '+[char]0x2014+' '+$title
  $request=@{mode='save';title=$title;initial_directory=(Native $corpus);filename=(Native $name)}
  $info=[Diagnostics.ProcessStartInfo]::new($app,'picker --cancel-on-disconnect --renderer opengl')
  $info.UseShellExecute=$false;$info.CreateNoWindow=$true;$info.RedirectStandardInput=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true
  $process=[Diagnostics.Process]::Start($info)
  try{
   $stdout=$process.StandardOutput.ReadToEndAsync();$stderr=$process.StandardError.ReadToEndAsync()
   $process.StandardInput.AutoFlush=$true;$process.StandardInput.WriteLine(($request|ConvertTo-Json -Depth 10 -Compress))
   WaitUntil {if($process.HasExited){throw ('Picker exited before showing a window: '+$stderr.Result)};[PickerNative]::Window($process.Id,$nativeTitle)-ne[IntPtr]::Zero} 'installed picker window'
   $window=[PickerNative]::Window($process.Id,$nativeTitle);[PickerNative]::Place($window);[PickerNative]::Focus($window)
   InvokeButton $window 'Save'
   $null=Node $window 'Replace' ([Windows.Automation.ControlType]::Button)
   Require ((EditCount $window)-eq0) 'Location editing was active before input'
   Key $window '^l'
   Require (-not$process.HasExited) 'Ctrl+L unexpectedly closed the picker'
   Require ((ReplaceCount $window)-eq1) 'Ctrl+L unexpectedly dismissed Replace'
   Key $window '{ESC}'
   WaitUntil {if($process.HasExited){throw 'Escape closed the entire picker'};(ReplaceCount $window)-eq0} 'Escape dismissed Replace'
   Require ((EditCount $window)-eq0) 'Ctrl+L enabled Location behind Replace'
   Key $window '^l'
   WaitUntil {(EditCount $window)-eq1} 'Ctrl+L works after Replace dismissal'
   Key $window '{ESC}'
   WaitUntil {if($process.HasExited){throw 'Escape closed the picker while editing Location'};(EditCount $window)-eq0} 'Escape dismissed Location editing'
   InvokeButton $window 'Save'
   $null=Node $window 'Replace' ([Windows.Automation.ControlType]::Button)
   Start-Sleep -Milliseconds 450
   [PickerNative]::Shot($window,(Join-Path $out ($variant.ToLowerInvariant()+'-confirmation.png')))
   Require ((Get-FileHash -LiteralPath $original).Hash-eq$originalHash) 'File changed before confirmation'
   InvokeButton $window 'Replace'
   Require ($process.WaitForExit(15000)) 'Picker did not finish after Replace'
   Require ($process.ExitCode-eq0) ('Picker failed: '+$stderr.Result)
   $response=$stdout.Result|ConvertFrom-Json
   Require ($response.accepted-and$response.paths.Count-eq1) 'Picker did not return one accepted path'
   $units=$response.paths[0].units_hex
   Require ($units-eq(Native $original).units_hex-or$units-eq(Native ('\\?\'+$original)).units_hex) 'Picker returned the wrong native path'
   Require ((Get-FileHash -LiteralPath $original).Hash-eq$originalHash) 'Picker wrote the caller file'
   Require ((Get-FileHash $configFile).Hash-eq$configHash) 'Picker changed main configuration'
   $report.cases+=@{variant=$variant;accepted=$true;originalUnchanged=$true;configUnchanged=$true;nativePathVerified=$true;modalShortcutOwnership=$true}
   Save
  }finally{StopOwned $process}
 }
 Phase 'uninstall and preserve caller file'
 $uninstaller=Join-Path $out 'owned-uninstaller.exe';Copy-Item (Join-Path $install 'Uninstall.exe') $uninstaller
 $null=Run $uninstaller ('/S _?='+$install) 'uninstall'
 Remove-Item -LiteralPath $uninstaller
 Require (-not(Test-Path $app)) 'Installed executable survived uninstall'
 Require ((Get-FileHash -LiteralPath $original).Hash-eq$originalHash) 'Uninstall changed the caller file'
 $report.status='pass';$report.phase='complete';$report.source=$packageInput.source_revision;Save
}catch{
 $report.status='failed';$report.error=($_|Out-String)+$_.ScriptStackTrace;Save;throw
}
