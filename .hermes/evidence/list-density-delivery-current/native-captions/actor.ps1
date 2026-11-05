param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExpectedSid)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$out=Join-Path $Root 'results'
$report=@{status='running';scope='Exact NSIS, ordinary-user Native renderer; caption/default-view, no-input footer, real Task Scheduler startup controls and uninstall preservation. Full theme, MSIX lifecycle and physical graphics remain separate.';cases=@()}
$owned=[Collections.Generic.List[Diagnostics.Process]]::new();$install=$null;$corpusOwned=$false
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
function InvokeButton($window,$name){(Node $window $name ([Windows.Automation.ControlType]::Button)).GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke();Pump}
function Window($process,$title){$script:window=[IntPtr]::Zero;WaitUntil {$script:window=[PickerNative]::Window($process.Id,$title);$script:window-ne[IntPtr]::Zero} ('owned '+$title);return $script:window}
function Launch($case){
 $config=Get-Content (Join-Path $Root 'config-template.json') -Raw|ConvertFrom-Json
 $config.bookmarks=@();$config.theme=$case.theme;$config.theme_preset='Ottrin';$config.default_view_mode=$case.view;$config.miller_view_scale=1.0;$config.list_view_scale=1.0;$config.grid_view_scale=1.0;switch($case.view){'Miller'{$config.miller_view_scale=$case.scale};'List'{$config.list_view_scale=$case.scale};'Grid'{$config.grid_view_scale=$case.scale}};$config.window_size=@(900,600);$config.graphics_renderer='Native';$config.custom_themes=@();$config.active_custom_theme=$null
 $config.search.include_roots=@($corpus);$config.search.content_indexing_enabled=$false
 [IO.File]::WriteAllText($pref,($config|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false))
 $info=[Diagnostics.ProcessStartInfo]::new($app,('"'+$corpus+'"'));$info.UseShellExecute=$false
 $script:current=[Diagnostics.Process]::Start($info);$owned.Add($script:current)
 $window=Window $script:current 'Ottrin';[PickerNative]::Place($window)
 $expected=@{Miller='Switch to List view';List='Switch to Grid view';Grid='Switch to Column view'}[$case.view]
 $null=Node $window $expected ([Windows.Automation.ControlType]::Button)
 WaitUntil {@(All $window|Where-Object {$_.Current.Name.Contains($wide)}).Count-gt0} 'wide filename displayed'
 $null=Node $window '3 items' ([Windows.Automation.ControlType]::Text)
 return $window
}
function CloseMain($window,$view){
 InvokeButton $window 'Close window';Require ($script:current.WaitForExit(15000)-and$script:current.ExitCode-eq0) 'Normal main close failed'
 $saved=Get-Content $pref -Raw|ConvertFrom-Json;Require ($saved.default_view_mode-eq$view) 'Default view preference changed on close'
}
function StartupEnabled{
 $service=New-Object -ComObject Schedule.Service;$service.Connect();$task=Get-OttrinIndexTask ($service.GetFolder('\'))
 Require ($null-ne$task) 'Installed caller-bound task is missing';Assert-OttrinTaskOwner $task;return [bool]$task.Enabled
}
# Frozen production task identity helpers; no independently reconstructed task name.
# Shared by the application's embedded scheduler scripts and the MSI transaction.
function Get-OttrinIndexTaskName {
    'Ottrin Index ' + [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
}

function Get-OttrinIndexTask($folder) {
    try { return $folder.GetTask((Get-OttrinIndexTaskName)) }
    catch {
        if ($_.Exception.GetBaseException().HResult -eq -2147024894) { return $null }
        throw
    }
}

function Get-OttrinTaskOwner($task) {
    $principal = $task.Definition.Principal.UserId
    if ($principal.StartsWith('S-1-')) { return $principal }
    ([Security.Principal.NTAccount]::new($principal)).Translate([Security.Principal.SecurityIdentifier]).Value
}

function Assert-OttrinTaskOwner($task) {
    if ((Get-OttrinTaskOwner $task) -ne [Security.Principal.WindowsIdentity]::GetCurrent().User.Value) {
        throw 'The existing index task belongs to another user; setup was not changed.'
    }
    if ($task.Definition.Principal.LogonType -ne 3) {
        throw 'The existing index task uses an unsupported logon type; setup was not changed.'
    }
}

# Caller first asks the authenticated daemon to exit gracefully when available.
# Scheduler stop also handles prior one-shot tasks that have no daemon endpoint.
function Stop-OttrinIndexTask($task) {
    Assert-OttrinTaskOwner $task
    $task.Stop(0)
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while ($task.GetInstances(0).Count -ne 0 -or $task.State -eq 4) {
        if ([DateTime]::UtcNow -ge $deadline) {
            throw 'The index task did not stop within 10 seconds; its registration was retained.'
        }
        Start-Sleep -Milliseconds 20
    }
}

# Retirement of the superseded registration is an upgrade operation only.
# Status/run/remove never fall back to the old global namespace.
function Get-OttrinSupersededTask($folder) {
    try { $task = $folder.GetTask('Ottrin Index') }
    catch {
        $code = $_.Exception.GetBaseException().HResult
        if ($code -eq -2147024894 -or $code -eq -2147024891) { return $null }
        throw
    }
    if ((Get-OttrinTaskOwner $task) -ne [Security.Principal.WindowsIdentity]::GetCurrent().User.Value) { return $null }
    $action = $task.Definition.Actions.Item(1)
    $leaf = [IO.Path]::GetFileName($action.Path)
    if ($leaf -notin @('ottrin.exe', 'ottrin-app.exe', 'ottrin-indexd.exe') -or
        $action.Arguments -notin @('index update', '--index-update')) { return $null }
    Assert-OttrinTaskOwner $task
    $task
}

function Get-OttrinTaskRecord($name, $task) {
    @{ name = $name; xml = $(if ($null -ne $task) { $task.Xml } else { $null }); security = $(if ($null -ne $task) { $task.GetSecurityDescriptor(7) } else { $null }); running = ($null -ne $task -and $task.GetInstances(0).Count -ne 0) }
}

function Restore-OttrinTaskRecord($folder, $record) {
    if ($record.running -isnot [bool]) { throw 'Rollback task running state is invalid.' }
    $existing = $null
    try { $existing = $folder.GetTask($record.name) }
    catch {
        if ($_.Exception.GetBaseException().HResult -ne -2147024894) { throw }
    }
    if ($null -ne $existing) { Assert-OttrinTaskOwner $existing }
    $restored = $null
    if ($null -ne $existing -and $existing.Xml -eq $record.xml -and
        $existing.GetSecurityDescriptor(7) -eq $record.security) {
        # A cleanup failure can leave the original task untouched. Preserve its
        # live instance instead of stopping and replacing an identical task.
        $restored = $existing
    } else {
        if ($null -ne $existing) { Stop-OttrinIndexTask $existing }
        if ($null -eq $record.xml) {
            if ($null -ne $existing) { $folder.DeleteTask($record.name, 0) }
        } else {
            $restored = $folder.RegisterTask($record.name, $record.xml, 54, $null, $null, 3, $null)
            $restored.SetSecurityDescriptor($record.security, 16)
        }
    }
    if ($null -ne $restored) { Restore-OttrinTaskRunningState $restored $record.running }
}

# Use current task metadata when preserving runtime intent after a successful
# upgrade; rollback first restores the captured metadata, then uses this too.
function Restore-OttrinTaskRunningState($task, [bool]$running) {
    Assert-OttrinTaskOwner $task
    if ($running -and $task.GetInstances(0).Count -eq 0) {
        # Resume captured instance intent, independently of future triggers.
        # Disabled tasks can have live instances; preserve that distinction.
        $enabled = $task.Enabled
        try {
            if (-not $enabled) { $task.Enabled = $true }
            $instance = $task.RunEx($null, 2, 0, $null)
            if ($null -eq $instance) { throw 'Restored index task did not accept a restart.' }
            $deadline = [DateTime]::UtcNow.AddSeconds(10)
            while ($instance.State -ne 4) {
                $instance.Refresh()
                if ([DateTime]::UtcNow -ge $deadline) { throw 'Restored index task did not start within 10 seconds.' }
                Start-Sleep -Milliseconds 20
            }
        } finally {
            if (-not $enabled) { $task.Enabled = $false }
        }
    } elseif (-not $running -and $task.GetInstances(0).Count -ne 0) {
        Stop-OttrinIndexTask $task
    }
}

try{
 Require ([Security.Principal.WindowsIdentity]::GetCurrent().User.Value-eq$ExpectedSid) 'Wrong owned account'
 Require (-not([Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Ordinary token required'
 $identity=Get-Content (Join-Path $Root 'application-input.json') -Raw|ConvertFrom-Json
 foreach($file in $identity.files){Require ((Get-FileHash (Join-Path $Root $file.name)).Hash.ToLowerInvariant()-eq$file.sha256) 'Native input mismatch'}
 $report.package=$identity;$manifest=Get-Content (Join-Path $Root 'payload.json') -Raw|ConvertFrom-Json
 $install=Join-Path $env:LOCALAPPDATA 'Programs\Ottrin';$app=Join-Path $install 'bin\ottrin.exe';Require (-not(Test-Path $install)) 'Existing installation retained'
 Phase 'install and verify all payloads';$null=Run (Join-Path $Root 'current-setup.exe') '/S' 'install'
 foreach($file in $manifest.PSObject.Properties){Require ((Get-FileHash (Join-Path $install $file.Name)).Hash.ToLowerInvariant()-eq$file.Value) ('Payload mismatch: '+$file.Name)}
 $null=Run $app 'index service-remove' 'initial-service-remove'
 $corpus=Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'OttrinCaptions';Require (-not(Test-Path $corpus)) 'Existing corpus retained'
 $null=[IO.Directory]::CreateDirectory($corpus);$corpusOwned=$true
 $wide='00-WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW.txt';$narrow='01-iiiiiiiiiiiiiiiiiiii.txt';$originals=@{}
 foreach($name in @($wide,$narrow,'preserve.txt')){$path=Join-Path $corpus $name;[IO.File]::WriteAllText($path,'owned original',[Text.UTF8Encoding]::new($false));$originals[$name]=(Get-FileHash $path).Hash}
 $pref=Join-Path $env:APPDATA 'ottrin\config.json';Require (-not(Test-Path $pref)) 'Existing preferences retained';$null=[IO.Directory]::CreateDirectory((Split-Path $pref))
 $cases=@()
 foreach($theme in @('Dark','Light')){foreach($view in @('Miller','List','Grid')){foreach($scale in @(.75,1.0,1.85)){$cases+=@{theme=$theme;view=$view;scale=$scale}}}}
 foreach($case in $cases){
  $label=($case.view+'-'+$case.theme+'-'+$case.scale);Phase $label;$window=Launch $case
  if($report.cases.Count-eq0){
   $clock=[Diagnostics.Stopwatch]::StartNew();$samples=@()
   foreach($seconds in @(0,2,10)){while($clock.Elapsed.TotalSeconds-lt$seconds){Pump};$null=Node $window '3 items' ([Windows.Automation.ControlType]::Text);$samples+=@{seconds=$clock.Elapsed.TotalSeconds;label='3 items'}}
   $report.no_input_footer_samples=$samples
  }
  Start-Sleep -Milliseconds 700;[PickerNative]::Shot($window,(Join-Path $out ($label+'.png')))
  $captions=@(All $window|Where-Object {$_.Current.Name.Contains($wide)-or$_.Current.Name.Contains($narrow)}|ForEach-Object {@{name=$_.Current.Name;type=$_.Current.ControlType.ProgrammaticName;width=$_.Current.BoundingRectangle.Width;height=$_.Current.BoundingRectangle.Height}})
  $report.cases+=@{theme=$case.theme;view=$case.view;view_scale=$case.scale;geometry=[PickerNative]::Geometry($window);capture=$label+'.png';captions=$captions;actual_view_toggle_verified=$true}
  CloseMain $window $case.view;Save
 }
 $densityProof=@()
 foreach($theme in @('Dark','Light')){foreach($view in @('Miller','List','Grid')){
  $samples=@($report.cases|Where-Object {$_.theme-eq$theme-and$_.view-eq$view}|Sort-Object {[double]$_.view_scale})
  Require ($samples.Count-eq3) 'Missing own-density case'
  $heights=@($samples|ForEach-Object {Require ($_.captions.Count-eq2) 'Expected wide and narrow actual file widgets';[double]$_.captions[0].height})
  $densityObserved=@{theme=$theme;view=$view;heights=$heights;scales=@($samples|ForEach-Object {$_.view_scale})}
  $report.density_observation=$densityObserved;Save
  Require (($heights.Count-eq3)-and(0-lt$heights[0])-and($heights[0]-lt$heights[1])-and($heights[1]-lt$heights[2])) ('Own density did not change actual '+$view+' widgets in '+$theme+': '+($densityObserved|ConvertTo-Json -Compress))
  $densityProof+=@{theme=$theme;view=$view;heights=$heights;scales=@($samples|ForEach-Object {$_.view_scale})}
 }}
 $report.density_proof=$densityProof;Save
 Phase 'real startup On Off On without stopping daemon';$null=Run $app 'index service-install' 'service-install'
 Require (StartupEnabled) 'Installed startup must be enabled';$case=@{theme='Dark';view='Miller';scale=1.0};$window=Launch $case
 InvokeButton $window 'Manage index service';$settings=Window $script:current 'Ottrin Settings';[PickerNative]::Place($settings)
 $null=Node $settings 'Automatic startup at sign-in: On' ([Windows.Automation.ControlType]::Text)
 $null=Node $settings ('Running '+[char]0x2014+' index ready') ([Windows.Automation.ControlType]::Text)
 $daemons=@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue);Require ($daemons.Count-eq1) 'Expected one owned index daemon';$daemonPid=$daemons[0].Id
 [PickerNative]::Shot($settings,(Join-Path $out 'services-on.png'))
 $toggle=Node $settings 'Start index automatically at sign-in' ([Windows.Automation.ControlType]::CheckBox)
 $toggle.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern).Toggle();WaitUntil {-not(StartupEnabled)} 'OS startup disabled'
 $null=Node $settings 'Automatic startup at sign-in: Off' ([Windows.Automation.ControlType]::Text)
 Require (@(Get-Process ottrin-indexd|Where-Object {$_.Id-eq$daemonPid}).Count-eq1) 'Startup toggle stopped daemon'
 [PickerNative]::Shot($settings,(Join-Path $out 'services-off.png'))
 (Node $settings 'Start index automatically at sign-in' ([Windows.Automation.ControlType]::CheckBox)).GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern).Toggle();WaitUntil {StartupEnabled} 'OS startup enabled again'
 $null=Node $settings 'Automatic startup at sign-in: On' ([Windows.Automation.ControlType]::Text)
 InvokeButton $settings 'Close window';CloseMain $window 'Miller'
 Require (@(Get-Process ottrin-indexd|Where-Object {$_.Id-eq$daemonPid}).Count-eq1) 'Normal GUI close stopped daemon'
 $report.service_round_trip=@{registration='On -> Off -> On';os_task_inspected=$true;running_pid_preserved=$daemonPid;normal_close_keeps_daemon=$true}
 $savedHash=(Get-FileHash $pref).Hash;$null=Run $app 'index service-remove' 'final-service-remove'
 Phase 'uninstall preserves preferences and files';$uninstaller=Join-Path $out 'owned-uninstall.exe';Copy-Item (Join-Path $install 'Uninstall.exe') $uninstaller
 $null=Run $uninstaller ('/S _?='+$install) 'uninstall';Remove-Item $uninstaller
 Require (-not(Test-Path $install)) 'Installation remains';Require ((Get-FileHash $pref).Hash-eq$savedHash) 'Preferences changed on uninstall'
 foreach($name in $originals.Keys){Require ((Get-FileHash (Join-Path $corpus $name)).Hash-eq$originals[$name]) 'Source file changed'}
 $report.preferences_preserved=$true;$report.sources_preserved=$true;$report.status='pass native caption/default-view, service startup and uninstall scope';Save
}catch{
 $report.status='failed';$report.error=$_.Exception.Message;$report.stack=$_.ScriptStackTrace
 if($script:current-and-not$script:current.HasExited){try{$failure=[PickerNative]::AnyWindow($script:current.Id);if($failure-ne[IntPtr]::Zero){[PickerNative]::Shot($failure,(Join-Path $out 'failure-window.png'));$report.failure_controls=@(All $failure|ForEach-Object {@{name=$_.Current.Name;type=$_.Current.ControlType.ProgrammaticName;enabled=$_.Current.IsEnabled}})}}catch{$report.capture_error=$_.Exception.Message}}
}finally{
 try{
  foreach($process in $owned){if(-not$process.HasExited){& "$env:SystemRoot\System32\taskkill.exe" /PID $process.Id /T /F|Out-Null;$null=$process.WaitForExit(10000)}}
  if($install-and(Test-Path $app)-and$manifest-and(Get-FileHash $app).Hash.ToLowerInvariant()-eq$manifest.'bin/ottrin.exe'){try{$null=Run $app 'index service-remove' 'cleanup-service-remove'}catch{$report.cleanup_error=$_.Exception.Message;$report.status='failed teardown'}}
  if($install){$left=@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath-and$_.ExecutablePath.StartsWith($install,[StringComparison]::OrdinalIgnoreCase)});Require ($left.Count-eq0) 'Owned process remains'}
  if($corpusOwned){Remove-Item -LiteralPath $corpus -Recurse -Force}
 }catch{$report.status='failed teardown';$report.cleanup_error=$_.Exception.Message}
 finally{Save}
}
if(-not$report.status.StartsWith('pass')){exit 1}
