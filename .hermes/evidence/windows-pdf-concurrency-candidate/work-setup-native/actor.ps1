param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExpectedSid)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$out=Join-Path $Root 'results'
$result=@{status='running';scope='Exact ordinary-user setup, index, rename and Inspector image-to-PDF'};$finalStatus='failed native work setup'
function Require($value,$message){if(-not$value){throw $message}}
function Save {$temporary=Join-Path $out 'RESULT.next';[IO.File]::WriteAllText($temporary,($result|ConvertTo-Json -Depth 25),[Text.UTF8Encoding]::new($false));$destination=Join-Path $out 'RESULT.json';if(Test-Path $destination){[IO.File]::Replace($temporary,$destination,[NullString]::Value)}else{[IO.File]::Move($temporary,$destination)}}
function Phase($name){$result.phase=$name;Save}
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

function TestInstalledPreviewTransition($binary, $pref, $selection) {
    $invoke = [Windows.Automation.InvokePattern]::Pattern
    $original = [IO.File]::ReadAllBytes($pref)
    $config = [Text.Encoding]::UTF8.GetString($original).TrimStart([char]0xfeff) | ConvertFrom-Json
    $config.default_view_mode = 'List'
    $config.window_size = @(1024, 680)
    $files = @('preview-image.png', 'preview.pdf', 'preview-second.pdf')
    $inputs = @{}
    foreach ($name in $files) {
        $path = Join-Path $selection $name
        Require (-not (Test-Path -LiteralPath $path)) "Unexpected existing fixture '$name'"
        $inputs[$name] = $path
    }
    $bitmap = [Drawing.Bitmap]::new(80, 60)
    try {
        $graphics = [Drawing.Graphics]::FromImage($bitmap)
        try {
            $graphics.Clear([Drawing.Color]::DarkGreen)
            $graphics.FillRectangle([Drawing.Brushes]::Gold, 20, 15, 40, 30)
        } finally { $graphics.Dispose() }
        $bitmap.Save($inputs['preview-image.png'], [Drawing.Imaging.ImageFormat]::Png)
    } finally { $bitmap.Dispose() }
    [IO.File]::WriteAllBytes($inputs['preview.pdf'], [Convert]::FromBase64String('JVBERi0xLjQKMSAwIG9iago8PCAvVHlwZSAvQ2F0YWxvZyAvUGFnZXMgMiAwIFIgPj4KZW5kb2JqCjIgMCBvYmoKPDwgL1R5cGUgL1BhZ2VzIC9LaWRzIFszIDAgUiA2IDAgUl0gL0NvdW50IDIgPj4KZW5kb2JqCjMgMCBvYmoKPDwgL1R5cGUgL1BhZ2UgL1BhcmVudCAyIDAgUiAvTWVkaWFCb3ggWzAgMCA2MTIgNzkyXSAvUmVzb3VyY2VzIDw8IC9Gb250IDw8IC9GMSA1IDAgUiA+PiA+PiAvQ29udGVudHMgNCAwIFIgPj4KZW5kb2JqCjQgMCBvYmoKPDwgL0xlbmd0aCA0NSA+PgpzdHJlYW0KQlQgL0YxIDE4IFRmIDcyIDcyMCBUZCAoT3R0cmluIFBERiBRQSkgVGogRVQKZW5kc3RyZWFtCmVuZG9iago1IDAgb2JqCjw8IC9UeXBlIC9Gb250IC9TdWJ0eXBlIC9UeXBlMSAvQmFzZUZvbnQgL0hlbHZldGljYSA+PgplbmRvYmoKNiAwIG9iago8PCAvVHlwZSAvUGFnZSAvUGFyZW50IDIgMCBSIC9NZWRpYUJveCBbMCAwIDYxMiA3OTJdIC9SZXNvdXJjZXMgPDwgL0ZvbnQgPDwgL0YxIDUgMCBSID4+ID4+IC9Db250ZW50cyA3IDAgUiA+PgplbmRvYmoKNyAwIG9iago8PCAvTGVuZ3RoIDU0ID4+CnN0cmVhbQpCVCAvRjEgMTggVGYgNzIgNzIwIFRkIChPdHRyaW4gUERGIFFBIC0gcGFnZSAyKSBUaiBFVAplbmRzdHJlYW0KZW5kb2JqCnhyZWYKMCA4CjAwMDAwMDAwMDAgNjU1MzUgZiAKMDAwMDAwMDAwOSAwMDAwMCBuIAowMDAwMDAwMDU4IDAwMDAwIG4gCjAwMDAwMDAxMjEgMDAwMDAgbiAKMDAwMDAwMDI0NyAwMDAwMCBuIAowMDAwMDAwMzQxIDAwMDAwIG4gCjAwMDAwMDA0MTEgMDAwMDAgbiAKMDAwMDAwMDUzNyAwMDAwMCBuIAp0cmFpbGVyCjw8IC9TaXplIDggL1Jvb3QgMSAwIFIgPj4Kc3RhcnR4cmVmCjY0MAolJUVPRgo='))
    [IO.File]::WriteAllBytes($inputs['preview-second.pdf'], [Convert]::FromBase64String('JVBERi0xLjQKMSAwIG9iago8PCAvVHlwZSAvQ2F0YWxvZyAvUGFnZXMgMiAwIFIgPj4KZW5kb2JqCjIgMCBvYmoKPDwgL1R5cGUgL1BhZ2VzIC9LaWRzIFszIDAgUl0gL0NvdW50IDEgPj4KZW5kb2JqCjMgMCBvYmoKPDwgL1R5cGUgL1BhZ2UgL1BhcmVudCAyIDAgUiAvTWVkaWFCb3ggWzAgMCA2MTIgNzkyXSAvUmVzb3VyY2VzIDw8IC9Gb250IDw8IC9GMSA1IDAgUiA+PiA+PiAvQ29udGVudHMgNCAwIFIgPj4KZW5kb2JqCjQgMCBvYmoKPDwgL0xlbmd0aCA2MSA+PgpzdHJlYW0KQlQgL0YxIDE4IFRmIDcyIDcyMCBUZCAoT3R0cmluIFBERiBRQSBzZWNvbmQgZG9jdW1lbnQpIFRqIEVUCmVuZHN0cmVhbQplbmRvYmoKNSAwIG9iago8PCAvVHlwZSAvRm9udCAvU3VidHlwZSAvVHlwZTEgL0Jhc2VGb250IC9IZWx2ZXRpY2EgPj4KZW5kb2JqCnhyZWYKMCA2CjAwMDAwMDAwMDAgNjU1MzUgZiAKMDAwMDAwMDAwOSAwMDAwMCBuIAowMDAwMDAwMDU4IDAwMDAwIG4gCjAwMDAwMDAxMTUgMDAwMDAgbiAKMDAwMDAwMDI0MSAwMDAwMCBuIAowMDAwMDAwMzUxIDAwMDAwIG4gCnRyYWlsZXIKPDwgL1NpemUgNiAvUm9vdCAxIDAgUiA+PgpzdGFydHhyZWYKNDIxCiUlRU9GCg=='))
    $hashes = @{}
    foreach ($name in $files) { $hashes[$name] = (Get-FileHash -LiteralPath $inputs[$name]).Hash }
    $process = $null
    try {
        [IO.File]::WriteAllText($pref, ($config | ConvertTo-Json -Depth 60), [Text.UTF8Encoding]::new($false))
        $process = [Diagnostics.Process]::Start([Diagnostics.ProcessStartInfo]::new($binary, ('"' + $selection + '"')))
        $main = WaitThemeWindow $process 'Ottrin'
        foreach ($name in $files) {
            Phase ('installed Inspector ' + $name)
            ClickDensityControl $main $name
            (OpenThemeControl $main 'Menu' $invoke).Invoke()
            (OpenThemeControl $main 'Quick Inspector Selection' $invoke).Invoke()
            $inspector = WaitThemeWindow $process ("Ottrin Quick Inspector " + [char]0x2014 + " " + $name)
            $sizing = if ($name.EndsWith('.pdf')) { 'Fit width' } else { 'Fit page' }
            $null = OpenThemeControl $inspector $sizing $invoke
            $null = OpenThemeControl $inspector '100%' $invoke
            [PickerNative]::Focus($inspector.window)
            Start-Sleep -Seconds 2
            Require (-not $process.HasExited) "Preview process exited during '$name'"
            [PickerNative]::Shot($inspector.window, (Join-Path $out ($name + '.png')))
            (OpenThemeControl $inspector 'Close window' $invoke).Invoke()
            $deadline = [DateTime]::UtcNow.AddSeconds(10)
            while ([PickerNative]::Window($process.Id, $inspector.title) -ne [IntPtr]::Zero) {
                Require (-not $process.HasExited -and [DateTime]::UtcNow -lt $deadline) "Inspector failed to close for '$name'"
                Start-Sleep -Milliseconds 100
            }
        }
        (OpenThemeControl $main 'Close window' $invoke).Invoke()
        Require ($process.WaitForExit(15000) -and $process.ExitCode -eq 0) 'Preview process failed normal close'
        foreach ($name in $files) {
            Require ((Get-FileHash -LiteralPath $inputs[$name]).Hash -eq $hashes[$name]) "Preview changed '$name'"
        }
        return @{files=$files; unchanged=$true; same_process=$true; normal_exit=$true}
    } finally {
        if ($null -ne $process) {
            if (-not $process.HasExited) { $process.Kill(); $null = $process.WaitForExit(5000) }
            $process.Dispose()
        }
        [IO.File]::WriteAllBytes($pref, $original)
    }
}
function IndexCli([string]$arguments) {
    $start = [Diagnostics.ProcessStartInfo]::new($app,$arguments)
    $start.UseShellExecute=$false; $start.CreateNoWindow=$true
    $start.RedirectStandardOutput=$true; $start.RedirectStandardError=$true
    $process=[Diagnostics.Process]::Start($start)
    try {
        $stdout=$process.StandardOutput.ReadToEndAsync()
        $stderr=$process.StandardError.ReadToEndAsync()
        Require ($process.WaitForExit(30000)) "Index command timed out: $arguments"
        Require ($stdout.Wait(5000) -and $stderr.Wait(5000)) "Index command left output open: $arguments"
        Require ($process.ExitCode -eq 0) "Index command failed: $arguments, $($stderr.Result)"
        return ($stdout.Result | ConvertFrom-Json)
    } finally {
        if (-not $process.HasExited) {$process.Kill(); $null=$process.WaitForExit(5000)}
        $process.Dispose()
    }
}
function FindIndexed([string]$term) {
    $deadline=[DateTime]::UtcNow.AddSeconds(60)
    do {
        $status=IndexCli 'index status --json'
        if ($status.status -eq 'ready' -and $status.watcher_ready -and $status.watched_roots -eq 1 -and $status.indexed_items -eq 1) {
            $query=IndexCli "search $term --json"
            if ($query.total -eq 1) {return @{ready=$true; watched_roots=1; indexed_items=1; query=$term; total=$query.total}}
        }
        Start-Sleep -Milliseconds 500
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "Installed index did not return the owned file for '$term'"
}
function TestRename($binary, $folder, $source, $hash) {
    $invoke = [Windows.Automation.InvokePattern]::Pattern
    $target = Join-Path $folder 'afterrename.txt'
    Require (-not (Test-Path -LiteralPath $target)) 'Rename destination already exists'
    $process = [Diagnostics.Process]::Start([Diagnostics.ProcessStartInfo]::new($binary, ('"' + $folder + '"')))
    try {
        $main = WaitThemeWindow $process 'Ottrin'
        [PickerNative]::Place($main.window)
        ClickDensityControl $main 'beforeindex.txt'
        [Windows.Forms.SendKeys]::SendWait('{F2}')
        $null = FindDensityControl $main 'New name'
        [Windows.Forms.SendKeys]::SendWait('^a')
        [Windows.Forms.SendKeys]::SendWait('afterrename.txt')
        $deadline=[DateTime]::UtcNow.AddSeconds(12)
        do {
            $button=FindDensityControl $main 'Rename' ([Windows.Automation.ControlType]::Button)
            if ($button.Current.IsEnabled) {break}
            Start-Sleep -Milliseconds 100
        } while ([DateTime]::UtcNow -lt $deadline)
        Require $button.Current.IsEnabled 'Rename button did not enable'
        $button.GetCurrentPattern($invoke).Invoke()
        $deadline=[DateTime]::UtcNow.AddSeconds(15)
        while (-not (Test-Path -LiteralPath $target) -or (Test-Path -LiteralPath $source)) {
            Require (-not $process.HasExited -and [DateTime]::UtcNow -lt $deadline) 'Ottrin did not complete the rename'
            Start-Sleep -Milliseconds 100
        }
        Require ((Get-FileHash -LiteralPath $target).Hash -eq $hash) 'Rename changed fixture bytes'
        (OpenThemeControl $main 'Close window' $invoke).Invoke()
        Require ($process.WaitForExit(15000) -and $process.ExitCode -eq 0) 'Installed main window failed normal close'
        return @{source='beforeindex.txt'; target='afterrename.txt'; sha256=$hash.ToLowerInvariant(); normal_exit=$true}
    } finally {
        if (-not $process.HasExited) {$process.Kill(); $null=$process.WaitForExit(5000)}
        $process.Dispose()
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
 $install=$null; $ownsInstall=$false
 $os=Get-CimInstance Win32_OperatingSystem
 $result.windows=@{caption=$os.Caption;build=$os.BuildNumber}
 $previewRoot=Join-Path $profilePath 'OttrinOwnedPreviews'
 Require (-not (Test-Path -LiteralPath $previewRoot)) 'Existing preview folder must not be touched'
 $null=New-Item -ItemType Directory $previewRoot
 $file=Join-Path $selection 'beforeindex.txt'
 [IO.File]::WriteAllText($file,'fixture bytes')
 $fileHash=(Get-FileHash $file).Hash
 $config=Get-Content (Join-Path $Root 'config-template.json') -Raw|ConvertFrom-Json
 $config.bookmarks=@(); $config.search.include_roots=@($selection); $config.search.exclude_roots=@()
 $config.search.indexed_volumes=@(); $config.search.content_indexing_enabled=$false
 $config.default_view_mode='List'; $config.window_size=@(1024,680); $config.theme='Dark'
 $pref=Join-Path $env:APPDATA 'ottrin\config.json'
 Require (-not(Test-Path $pref)) 'Existing preferences must not be replaced'
 $null=[IO.Directory]::CreateDirectory((Split-Path $pref))
 [IO.File]::WriteAllText($pref,($config|ConvertTo-Json -Depth 60),[Text.UTF8Encoding]::new($false))
 Require (@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue).Count-eq0) 'Unexpected initial daemon'
 Phase 'install exact NSIS'
 $install=Join-Path $env:LOCALAPPDATA 'Programs\Ottrin'
 Require (-not(Test-Path $install)) 'Existing installation must not be touched'
 $ownsInstall=$true
 $installer=Start-Process (Join-Path $Root 'current-setup.exe') -ArgumentList '/S' -PassThru -Wait
 $result.install_exit=$installer.ExitCode
 Require ($installer.ExitCode-eq0) 'Current NSIS install failed'
 $app=Join-Path $install 'bin\ottrin.exe'
 foreach($entry in $manifest.PSObject.Properties){Require ((Get-FileHash (Join-Path $install $entry.Name)).Hash.ToLowerInvariant()-eq$entry.Value) ('Installed payload mismatch: '+$entry.Name)}
 Require (@(Get-ChildItem $install -Recurse -File).Count-eq(@($manifest.PSObject.Properties).Count+1)) 'Expected exact payload plus generated uninstaller'
 Require (-not(Test-Path ($install+'.ottrin-setup'))-and-not(Test-Path ($install+'.ottrin-completed'))) 'Completed setup transaction residue'
 Require (Test-Path -LiteralPath (Join-Path $install 'Uninstall.exe') -PathType Leaf) 'Installed uninstaller missing'
 $result.payload_files=@($manifest.PSObject.Properties).Count
 $result.installed_binary_sha256=(Get-FileHash $app).Hash.ToLowerInvariant()
 Add-Type -AssemblyName UIAutomationClient
 Add-Type -AssemblyName UIAutomationTypes
 Add-Type -AssemblyName System.Windows.Forms
 Phase 'launch installed app and switch from image to PDF'
 $result.installed_preview_transition=TestInstalledPreviewTransition $app $pref $previewRoot
 Phase 'build and query the scoped index'
 $null=IndexCli 'index update --json'
 $result.index_before=FindIndexed 'beforeindex'
 Phase 'rename an indexed file from installed Ottrin'
 $result.rename=TestRename $app $selection $file $fileHash
 Phase 'query the renamed file through the index watcher'
 $result.index_after=FindIndexed 'afterrename'
 $oldQuery=IndexCli 'search beforeindex --json'
 Require ($oldQuery.total -eq 0) 'Old filename remains in the index'
 $result.old_filename_removed=$true
 $daemon=Start-Process (Join-Path $install 'bin\ottrin-indexd.exe') -ArgumentList '--stop' -PassThru -Wait
 Require ($daemon.ExitCode -eq 0 -and @(Get-Process ottrin-indexd -ErrorAction SilentlyContinue).Count -eq 0) 'Installed index did not stop'
 $result.index_stopped=$true
 $finalStatus='pass'
}catch{$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally{
 if($ownsInstall-and(Test-Path (Join-Path $install 'Uninstall.exe'))){
  $uninstall=Join-Path $out 'owned-uninstall.exe'
  Copy-Item (Join-Path $install 'Uninstall.exe') $uninstall
  $removed=Start-Process $uninstall -ArgumentList ('/S _?='+$install) -PassThru -Wait
  $result.uninstall_exit=$removed.ExitCode
  Remove-Item $uninstall
 }
 $result.owned_installation_removed=($ownsInstall-and-not(Test-Path $install))
 if ($finalStatus -eq 'pass' -and (-not $result.owned_installation_removed -or $result.uninstall_exit -ne 0)) {
  $finalStatus='failed native uninstall'
 }
 $result.status=$finalStatus
 Save
}
