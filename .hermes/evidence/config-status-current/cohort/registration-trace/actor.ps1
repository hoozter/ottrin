param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExpectedSid)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$out=Join-Path $Root 'results';$report=@{status='running';scope='Diagnostic execution trace of unchanged production task registration script via EncodedCommand; not application UI or service acceptance';cases=@()};$final='failed native settings preservation workflow';$readonlyConfig=$null;$originalAttributes=$null;$owned=[Collections.Generic.List[Diagnostics.Process]]::new();$install=$null
function Require($value,$message){if(-not$value){throw $message}}
function Save{$p=Join-Path $out 'RESULT.json';$tmp=$p+'.next';[IO.File]::WriteAllText($tmp,($report|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false));if(Test-Path $p){[IO.File]::Replace($tmp,$p,[NullString]::Value)}else{[IO.File]::Move($tmp,$p)}}
function Phase($value){$report.phase=$value;Save}
function Run($exe,$arguments,$name,$expectedExit=0){
 $s=[Diagnostics.ProcessStartInfo]::new($exe,$arguments);$s.UseShellExecute=$false;$s.CreateNoWindow=$true;$s.RedirectStandardOutput=$true;$s.RedirectStandardError=$true;$s.StandardOutputEncoding=[Text.UTF8Encoding]::new($false);$s.StandardErrorEncoding=[Text.UTF8Encoding]::new($false);$clock=[Diagnostics.Stopwatch]::StartNew();$p=[Diagnostics.Process]::Start($s)
 try{
  $stdout=$p.StandardOutput.ReadToEndAsync();$stderr=$p.StandardError.ReadToEndAsync()
  if(-not$p.WaitForExit(180000)){
   $snapshot=@(Get-CimInstance Win32_Process|Where-Object {$_.ProcessId-eq$p.Id-or$_.ParentProcessId-eq$p.Id-or($install-and$_.ExecutablePath-and$_.ExecutablePath.StartsWith($install,[StringComparison]::OrdinalIgnoreCase))}|Select-Object Name,ProcessId,ParentProcessId,ExecutablePath,SessionId)
   [IO.File]::WriteAllText((Join-Path $out ($name+'-timeout.json')),(@{seconds=$clock.Elapsed.TotalSeconds;root_pid=$p.Id;processes=$snapshot}|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false));throw ('Owned process timeout '+$name)
  }
  Require ($stdout.Wait(5000)-and$stderr.Wait(5000)) 'Process streams not complete';$v=@{exit=$p.ExitCode;stdout=$stdout.Result;stderr=$stderr.Result;seconds=$clock.Elapsed.TotalSeconds};[IO.File]::WriteAllText((Join-Path $out ($name+'-process.json')),($v|ConvertTo-Json),[Text.UTF8Encoding]::new($false));Require ($v.exit-eq$expectedExit) ('Native child failed '+$name);return $v
 }finally{if(-not$p.HasExited){& "$env:SystemRoot\System32\taskkill.exe" /PID $p.Id /T /F|Out-Null;$null=$p.WaitForExit(10000)};$p.Dispose()}
}
& ([ScriptBlock]::Create([IO.File]::ReadAllText((Join-Path $Root 'native-controls.ps1'))));[PickerNative]::Init()
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
function Pump{[Windows.Forms.Application]::DoEvents();Start-Sleep -Milliseconds 50}
function Tree($w){return [Windows.Automation.AutomationElement]::FromHandle($w)}
function Nodes($w,$name){$c=[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,$name);return @((Tree $w).FindAll([Windows.Automation.TreeScope]::Descendants,$c)|Where-Object {-not$_.Current.IsOffscreen})}
function Node($w,$name,$kind){$deadline=[DateTime]::UtcNow.AddSeconds(20);do{$n=@(Nodes $w $name|Where-Object {$_.Current.ControlType-eq$kind});if($n.Count-eq1){return $n[0]};Pump}while([DateTime]::UtcNow-lt$deadline);[PickerNative]::Shot($w,(Join-Path $out 'missing-control.png'));throw ('Missing unique '+$name+' count='+$n.Count)}
function Invoke($w,$name){$n=Node $w $name ([Windows.Automation.ControlType]::Button);$n.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke();Pump}
function Key($w,$key){[PickerNative]::Focus($w);[Windows.Forms.SendKeys]::SendWait($key);Start-Sleep -Milliseconds 300;Pump}
try{
 Require ([Security.Principal.WindowsIdentity]::GetCurrent().User.Value-eq$ExpectedSid) 'Wrong owned account';Require (-not([Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Ordinary token required'
 $identity=Get-Content (Join-Path $Root 'application-input.json') -Raw|ConvertFrom-Json;foreach($file in $identity.files){Require ((Get-FileHash (Join-Path $Root $file.name)).Hash.ToLowerInvariant()-eq$file.sha256) 'Native actor input mismatch'};$report.package=$identity
 $manifest=Get-Content (Join-Path $Root 'payload.json') -Raw|ConvertFrom-Json;$install=Join-Path $env:LOCALAPPDATA 'Programs\Ottrin';$app=Join-Path $install 'bin\ottrin.exe';Require (-not(Test-Path $install)) 'Existing installation retained'
 Phase 'install exact staged NSIS';$null=Run (Join-Path $Root 'current-setup.exe') '/S' 'install'
 foreach($file in $manifest.PSObject.Properties){Require ((Get-FileHash (Join-Path $install $file.Name)).Hash.ToLowerInvariant()-eq$file.Value) ('Payload mismatch '+$file.Name)}
 $null=Run $app 'index service-remove' 'index-remove';Require (@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue).Count-eq0) 'Owned index process remains'
 $corpus=Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'OttrinFields';Require (-not(Test-Path $corpus)) 'Existing corpus retained';$null=[IO.Directory]::CreateDirectory($corpus);$source=Join-Path $corpus 'preserve.txt';[IO.File]::WriteAllText($source,'owned original',[Text.UTF8Encoding]::new($false));$sourceHash=(Get-FileHash $source).Hash
 $config=Get-Content (Join-Path $Root 'config-template.json') -Raw|ConvertFrom-Json;$config.bookmarks=@();$config.default_view_mode='List';$config.graphics_renderer='Native';$config.search.include_roots=@($corpus);$config.search.content_indexing_enabled=$false;$pref=Join-Path $env:APPDATA 'ottrin\config.json';Require (-not(Test-Path $pref)) 'Existing config retained';$null=[IO.Directory]::CreateDirectory((Split-Path $pref))




 $script=@'
Set-PSDebug -Trace 1
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

# Embedded by the search crate; task parameters arrive as process environment data.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$TaskName = Get-OttrinIndexTaskName
$executable = $env:OTTRIN_TASK_EXECUTABLE
if (-not [IO.File]::Exists($executable)) { throw 'The Ottrin executable does not exist.' }
$user = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$service = New-Object -ComObject Schedule.Service
$service.Connect()
$folder = $service.GetFolder('\')
$existing = Get-OttrinIndexTask $folder
$superseded = Get-OttrinSupersededTask $folder
$prior = if ($null -ne $existing) { $existing } else { $superseded }
$existingRecord = Get-OttrinTaskRecord $TaskName $existing
$supersededRecord = Get-OttrinTaskRecord 'Ottrin Index' $superseded
$enabled = $true
$security = $null
if ($null -ne $prior) {
    Assert-OttrinTaskOwner $prior
    $enabled = $prior.Enabled
    $security = $prior.GetSecurityDescriptor(7)
}
$definition = $service.NewTask(0)
$definition.RegistrationInfo.Description = 'Ottrin continuous background index service'
$definition.Principal.UserId = $user
$definition.Principal.LogonType = 3 # Interactive token, no stored password.
$definition.Principal.RunLevel = 0 # Least privilege.
$definition.Settings.Enabled = $enabled
$definition.Settings.AllowDemandStart = $true
$definition.Settings.MultipleInstances = 2 # Ignore a trigger while indexing runs.
$definition.Settings.ExecutionTimeLimit = 'PT0S' # Persistent daemon, no runtime limit.
$definition.Settings.DisallowStartIfOnBatteries = $false
$definition.Settings.StopIfGoingOnBatteries = $false
$trigger = $definition.Triggers.Create(9) # Start once when this user logs in.
$trigger.UserId = $user
$trigger.Enabled = $true
$action = $definition.Actions.Create(0)
$action.Path = $executable
$action.Arguments = '--index-daemon'
# Preserve the disabled state atomically and retain the existing permissions.
# CREATE_OR_UPDATE | DONT_ADD_PRINCIPAL_ACE | IGNORE_REGISTRATION_TRIGGERS.
if ($null -ne $existing) { Stop-OttrinIndexTask $existing }
$registered = $folder.RegisterTaskDefinition($TaskName, $definition, 54, $null, $null, 3, $security)
if ($null -ne $superseded) {
    try { $superseded.Enabled = $false; Stop-OttrinIndexTask $superseded; $folder.DeleteTask('Ottrin Index', 0) }
    catch {
        # A failed retirement must not leave two scheduled writers active.
        $registered.Enabled = $false
        Restore-OttrinTaskRecord $folder $supersededRecord
        Restore-OttrinTaskRecord $folder $existingRecord
        throw
    }
}
if ($enabled) { 'Background Ottrin index service enabled at login with Windows Task Scheduler.' }
else { 'Background Ottrin index task updated; automatic startup remains disabled.' }

'@

 [IO.File]::WriteAllText($pref,($config|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false))
 Phase 'trace unchanged task registration statements'
 $env:OTTRIN_TASK_EXECUTABLE=Join-Path $install 'bin\ottrin-indexd.exe'
 $encoded=[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($script))
 $info=[Diagnostics.ProcessStartInfo]::new('powershell.exe',('-NoProfile -NonInteractive -EncodedCommand '+$encoded));$info.UseShellExecute=$false;$info.CreateNoWindow=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true
 $clock=[Diagnostics.Stopwatch]::StartNew();$p=[Diagnostics.Process]::Start($info);$owned.Add($p);$stdout=$p.StandardOutput.ReadToEndAsync();$stderr=$p.StandardError.ReadToEndAsync()
 $completed=$p.WaitForExit(180000)
 if(-not$completed){& "$env:SystemRoot\System32\taskkill.exe" /PID $p.Id /T /F|Out-Null;$null=$p.WaitForExit(10000)}
 Require ($stdout.Wait(5000)-and$stderr.Wait(5000)) 'Trace streams did not finish'
 [IO.File]::WriteAllText((Join-Path $out 'task-trace.json'),(@{seconds=$clock.Elapsed.TotalSeconds;completed=$completed;exit=$p.ExitCode;stdout=$stdout.Result;stderr=$stderr.Result}|ConvertTo-Json -Depth 6),[Text.UTF8Encoding]::new($false))
 $report.trace=@{seconds=$clock.Elapsed.TotalSeconds;completed=$completed;exit=$p.ExitCode;transport='EncodedCommand with execution trace; source statements unchanged'};Save
 Require ($completed-and$p.ExitCode-eq0) 'Task script did not complete successfully; inspect task-trace.json'
 $scheduler=New-Object -ComObject Schedule.Service;$scheduler.Connect();$task=$scheduler.GetFolder('\').GetTask(('Ottrin Index '+$ExpectedSid));$report.registration=@{enabled=$task.Enabled;principal=$task.Definition.Principal.UserId;action=$task.Definition.Actions.Item(1).Path};Save
 $final='pass diagnostic registration script; application transport/native controls not accepted'
}catch{$report.error=$_.Exception.Message;$report.stack=$_.ScriptStackTrace}
finally{
 foreach($p in $owned){if(-not$p.HasExited){& "$env:SystemRoot\System32\taskkill.exe" /PID $p.Id /T /F|Out-Null;$null=$p.WaitForExit(10000)}}
 if($install-and(Test-Path $app)){
  try{$report.cleanup_index_remove=Run $app 'index service-remove' 'cleanup-index-remove'}catch{$report.cleanup_index_error=$_.Exception.Message;$final='failed owned trace teardown'}
 }
 $report.status=$final;Save
}
if(-not$final.StartsWith('pass')){exit 1}
