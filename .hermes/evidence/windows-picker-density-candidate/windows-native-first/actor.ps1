param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExpectedSid)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$out=Join-Path $Root 'results'
$result=@{status='running';scope='List density and overlapping preferences in exact portable and installed pickers';picker_cases=@()};$finalStatus='failed native folder picker'
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
# Run from the owned interactive picker actor after verifying the packaged binary.
# Run before tests that change the saved chooser view.
# The actor supplies LaunchPicker, WaitMapped, NativePath, Require and PickerNative.
function FindDensityControl($entry, [string]$name, $controlType = $null) {
    Require ([PickerNative]::Window($entry.process.Id, $entry.title) -eq $entry.window) 'Owned picker window changed'
    $tree = [Windows.Automation.AutomationElement]::FromHandle($entry.window)
    $condition = [Windows.Automation.PropertyCondition]::new(
        [Windows.Automation.AutomationElement]::NameProperty, $name)
    $deadline = [DateTime]::UtcNow.AddSeconds(12)
    do {
        $controls = @($tree.FindAll([Windows.Automation.TreeScope]::Descendants, $condition) |
            Where-Object { -not $_.Current.IsOffscreen -and -not $_.Current.BoundingRectangle.IsEmpty -and
                ($null -eq $controlType -or $_.Current.ControlType -eq $controlType) })
        if ($controls.Count -eq 1) {
            return $controls[0]
        }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "No unique visible picker control named '$name'"
}

function ClickDensityControl($entry, [string]$name) {
    [PickerNative]::Focus($entry.window)
    $control = FindDensityControl $entry $name
    Require $control.Current.IsEnabled "Picker control '$name' is disabled"
    $rect = $control.Current.BoundingRectangle
    $geometry = [PickerNative]::Geometry($entry.window)
    Require ($geometry -match '^client=(\d+)x(\d+) origin=(-?\d+),(-?\d+) dpi=(\d+)$') 'Unrecognized owned picker geometry'
    $x = $rect.X + $rect.Width / 2 - [int]$Matches[3]
    $y = $rect.Y + $rect.Height / 2 - [int]$Matches[4]
    Require ($x -gt 0 -and $x -lt [int]$Matches[1] -and $y -gt 0 -and $y -lt [int]$Matches[2]) 'Control center lies outside the owned picker'
    $scale = [int]$Matches[5] / 96.0
    [PickerNative]::Click($entry.window, $x / $scale, $y / $scale)
}

function TestPickerListDensity($binary, $pref, $selection, [string]$label) {
    $pickerPreferences = Join-Path (Split-Path $pref) 'picker.json'
    if (Test-Path -LiteralPath $pickerPreferences) {
        $preferences = Get-Content -LiteralPath $pickerPreferences -Raw | ConvertFrom-Json
        Require (-not $preferences.grid) 'Owned chooser must start in List view'
    }
    $alpha = Join-Path $selection 'alpha.txt'
    $beta = Join-Path $selection 'beta.txt'
    $original = [IO.File]::ReadAllBytes($pref)
    $created = [Collections.Generic.List[string]]::new()
    try {
        foreach ($path in @($alpha, $beta)) {
            $stream = [IO.File]::Open($path, [IO.FileMode]::CreateNew)
            $created.Add($path)
            $stream.Dispose()
            [IO.File]::WriteAllText($path, "Owned fixture: $path")
        }
        $fixtureHashes = @(@($alpha, $beta) | ForEach-Object { (Get-FileHash -LiteralPath $_).Hash })
        $config = [Text.Encoding]::UTF8.GetString($original).TrimStart([char]0xfeff) | ConvertFrom-Json
        $cases = @()
        foreach ($trial in @(@{name='compact'; scale=0.9}, @{name='large'; scale=1.35})) {
            $config | Add-Member -NotePropertyName list_view_scale -NotePropertyValue $trial.scale -Force
            [IO.File]::WriteAllText($pref, ($config | ConvertTo-Json -Depth 60), [Text.UTF8Encoding]::new($false))
            $hash = (Get-FileHash -LiteralPath $pref).Hash
            $entry = LaunchPicker $binary "$label-$($trial.name)" ''
            WaitMapped $entry
            [PickerNative]::Place($entry.window)
            $first = (FindDensityControl $entry 'alpha.txt').Current.BoundingRectangle
            $second = (FindDensityControl $entry 'beta.txt').Current.BoundingRectangle
            Require ($first.Width -eq $second.Width -and $first.Height -eq $second.Height -and
                     $first.X -eq $second.X -and $second.Y -ge $first.Bottom) 'Expected distinct aligned List rows'
            $null = FindDensityControl $entry 'Cancel'
            ClickDensityControl $entry 'beta.txt'
            ClickDensityControl $entry 'Open'
            Require ($entry.process.WaitForExit(12000)) 'Picker did not close after Open'
            Require ($entry.process.ExitCode -eq 0 -and $entry.stdout.Wait(5000)) 'Picker did not return a successful response'
            $response = $entry.stdout.Result | ConvertFrom-Json
            $expected = NativePath $beta
            Require ($response.accepted -and @($response.paths).Count -eq 1 -and
                     $response.paths[0].encoding -ceq $expected.encoding -and
                     $response.paths[0].units_hex -ceq $expected.units_hex) 'Picker did not open the exact clicked file'
            Require ((Get-FileHash -LiteralPath $pref).Hash -eq $hash) 'Picker changed main preferences'
            $cases += @{scale=$trial.scale; pitch=$second.Y-$first.Y; height=$first.Height; opened_expected_file=$true}
        }
        Require ($cases[1].pitch - $cases[0].pitch -ge 8 -and $cases[1].height -gt $cases[0].height) 'Saved List density did not change row geometry'
        Require (((Get-FileHash -LiteralPath $alpha).Hash -eq $fixtureHashes[0]) -and
                 ((Get-FileHash -LiteralPath $beta).Hash -eq $fixtureHashes[1])) 'Picker modified fixture bytes'
        return $cases
    } finally {
        [IO.File]::WriteAllBytes($pref, $original)
        foreach ($path in $created) {
            Remove-Item -LiteralPath $path
        }
    }
}

function WaitThemeWindow($process, [string]$title) {
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    do {
        Require (-not $process.HasExited) "Installed Ottrin exited before '$title' opened"
        $window = [PickerNative]::Window($process.Id, $title)
        if ($window -ne [IntPtr]::Zero) {
            return @{process=$process; title=$title; window=$window}
        }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "Missing owned window '$title'"
}

function OpenThemeControl($entry, [string]$name, $pattern) {
    $kind = if ($pattern -eq [Windows.Automation.InvokePattern]::Pattern) {
        [Windows.Automation.ControlType]::Button
    } else {
        [Windows.Automation.ControlType]::RadioButton
    }
    $control = FindDensityControl $entry $name $kind
    Require $control.Current.IsEnabled "Theme control '$name' is disabled"
    return $control.GetCurrentPattern($pattern)
}

function TestInstalledThemeWindows($binary, $pref, $selection) {
    $invoke = [Windows.Automation.InvokePattern]::Pattern
    $original = [IO.File]::ReadAllBytes($pref)
    $config = [Text.Encoding]::UTF8.GetString($original).TrimStart([char]0xfeff) | ConvertFrom-Json
    $config.window_size = @(1024, 680)
    $config.theme_preset = 'Ottrin'
    $config.active_custom_theme = $null
    $config.custom_themes = @()
    $cases = @()
    try {
        foreach ($theme in @('Dark', 'Light')) {
            $config.theme = $theme
            [IO.File]::WriteAllText($pref, ($config | ConvertTo-Json -Depth 60), [Text.UTF8Encoding]::new($false))
            $start = [Diagnostics.ProcessStartInfo]::new($binary, ('"' + $selection + '"'))
            $process = [Diagnostics.Process]::Start($start)
            try {
                $main = WaitThemeWindow $process 'Ottrin'
                (OpenThemeControl $main 'Menu' $invoke).Invoke()
                (OpenThemeControl $main ('Settings' + [char]0x2026) $invoke).Invoke()
                $settings = WaitThemeWindow $process 'Ottrin Settings'
                $select = [Windows.Automation.SelectionItemPattern]::Pattern
                (OpenThemeControl $settings 'Appearance' $select).Select()
                $deadline = [DateTime]::UtcNow.AddSeconds(12)
                do {
                    $tree = [Windows.Automation.AutomationElement]::FromHandle($settings.window)
                    $name = [Windows.Automation.PropertyCondition]::new(
                        [Windows.Automation.AutomationElement]::NameProperty, 'Customize Ottrin')
                    $controls = @($tree.FindAll([Windows.Automation.TreeScope]::Descendants, $name) |
                        Where-Object { -not $_.Current.IsOffscreen -and
                            $_.Current.ControlType -eq [Windows.Automation.ControlType]::Button })
                    if ($controls.Count -eq 1) { break }
                    [PickerNative]::Wheel($settings.window, -480)
                    Start-Sleep -Milliseconds 150
                } while ([DateTime]::UtcNow -lt $deadline)
                Require ($controls.Count -eq 1) 'Appearance did not expose Customize Ottrin'
                $controls[0].GetCurrentPattern($invoke).Invoke()
                $editor = WaitThemeWindow $process 'Ottrin Theme Editor'
                [PickerNative]::Place($editor.window)
                $null = FindDensityControl $editor 'Toggle Search'
                $null = FindDensityControl $editor 'Cancel'
                (OpenThemeControl $editor 'Menu' $invoke).Invoke()
                $null = FindDensityControl $editor 'New Folder'
                Require (-not (FindDensityControl $editor 'Paste into Current Folder').Current.IsEnabled) 'Preview must disable Paste'
                (OpenThemeControl $editor 'New Folder' $invoke).Invoke()
                $deadline = [DateTime]::UtcNow.AddSeconds(5)
                $tree = [Windows.Automation.AutomationElement]::FromHandle($editor.window)
                $newFolder = [Windows.Automation.PropertyCondition]::new(
                    [Windows.Automation.AutomationElement]::NameProperty, 'New Folder')
                do {
                    $remaining = @($tree.FindAll([Windows.Automation.TreeScope]::Descendants, $newFolder) |
                        Where-Object { -not $_.Current.IsOffscreen })
                    if ($remaining.Count -eq 0) { break }
                    Require ([DateTime]::UtcNow -lt $deadline) 'Sample action did not dismiss the preview menu'
                    Start-Sleep -Milliseconds 100
                } while ($true)
                ClickDensityControl $editor 'Cancel'
                $deadline = [DateTime]::UtcNow.AddSeconds(10)
                while ([PickerNative]::Window($process.Id, $editor.title) -ne [IntPtr]::Zero) {
                    Require ([DateTime]::UtcNow -lt $deadline) 'Clicking Cancel did not close Theme Editor'
                    Start-Sleep -Milliseconds 100
                }
                (OpenThemeControl $settings 'Close window' $invoke).Invoke()
                (OpenThemeControl $main 'Close window' $invoke).Invoke()
                Require ($process.WaitForExit(15000) -and $process.ExitCode -eq 0) 'Installed main window failed to close normally'
                $saved = Get-Content -LiteralPath $pref -Raw | ConvertFrom-Json
                Require ($saved.theme -eq $theme -and $null -eq $saved.active_custom_theme -and
                         $saved.custom_themes.Count -eq 0) 'Cancel changed the saved theme'
                $cases += @{theme=$theme; physical_cancel=$true; preview_menu=$true; normal_exit=$true}
            } finally {
                if (-not $process.HasExited) {
                    $process.Kill()
                    $null = $process.WaitForExit(5000)
                }
                $process.Dispose()
            }
        }
        return $cases
    } finally {
        [IO.File]::WriteAllBytes($pref, $original)
    }
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
 Phase 'portable and installed List density'
 $result.list_density=@{
  portable=@(TestPickerListDensity $portable $pref $selection 'portable')
  installed=@(TestPickerListDensity $app $pref $selection 'installed')
 }
 Require (-not(Test-Path $pickerPrefs)) 'Density checks unexpectedly wrote picker view preferences'
 Phase 'overlapping portable and installed picker preferences'
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
 $result.no_index=$true;$result.source_preserved=$true;$result.preferences_preserved=$true
 Phase 'installed Theme Editor in Dark and Light'
 $result.installed_theme=@(TestInstalledThemeWindows $app $pref $selection)
 $daemon=Start-Process (Join-Path $install 'bin\ottrin-indexd.exe') -ArgumentList '--stop' -PassThru -Wait
 Require ($daemon.ExitCode -eq 0 -and @(Get-Process ottrin-indexd -ErrorAction SilentlyContinue).Count -eq 0) 'Installed index remained after theme test'
 Require ((Get-FileHash $pref).Hash-eq$before -and (Get-FileHash $file).Hash-eq$fileHash) 'Installed theme test changed protected fixture'
 $finalStatus='pass'
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
