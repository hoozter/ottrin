$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$r='C:\OttrinQA-20261011\commit-current'
# Shared by the application's embedded scheduler scripts and the MSI transaction.
function Get-OttrinIndexTaskName {
    'Ottrin Commit QA ' + [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
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

# Isolate this task test from all legacy/production task names.
function Get-OttrinSupersededTask($folder) { return $null }
function Transaction($phase,$snapshot) {
 & {
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('Snapshot', 'Rollback', 'Activate', 'Commit')]
    [string]$Phase,
    [Parameter(Mandatory = $true)]
    [string]$Snapshot
)

# Installers own this short-lived transaction; the application owns normal task setup.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
# The embedded controller supplies the shared identity functions in parent scope.
$TaskName = Get-OttrinIndexTaskName

function Task-Folder {
    $service = New-Object -ComObject Schedule.Service
    $service.Connect()
    $service.GetFolder('\')
}

function Read-TransactionSnapshot {
    $record = Get-Content -LiteralPath $Snapshot -Encoding UTF8 -Raw | ConvertFrom-Json
    if ($record.version -ne 3 -or $record.user -ne [Security.Principal.WindowsIdentity]::GetCurrent().User.Value) {
        throw 'Index rollback snapshot does not belong to this user or schema.'
    }
    if (@($record.tasks).Count -lt 1 -or @($record.tasks).Count -gt 2) {
        throw 'Index rollback snapshot has an invalid task set.'
    }
    foreach ($taskRecord in $record.tasks) {
        if ($taskRecord.name -notin @($TaskName, 'Ottrin Index') -or $taskRecord.running -isnot [bool]) {
            throw 'Rollback task identity or running intent is outside this transaction.'
        }
    }
    return $record
}

if ($Phase -eq 'Snapshot') {
    if (Test-Path -LiteralPath $Snapshot) {
        # A failed restoration remains available for diagnosis/recovery. Do not
        # let this transaction's rollback consume an earlier transaction's data.
        $null = Read-TransactionSnapshot
        $retained = "$Snapshot.failed-$([Guid]::NewGuid().ToString('N'))"
        Move-Item -LiteralPath $Snapshot -Destination $retained
        throw "A previous index rollback snapshot requires attention: $retained"
    }
    $folder = Task-Folder
    $task = Get-OttrinIndexTask $folder
    if ($null -ne $task) { Assert-OttrinTaskOwner $task }
    $superseded = Get-OttrinSupersededTask $folder
    $record = @{ version = 3; user = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value; tasks = @((Get-OttrinTaskRecord $TaskName $task)) }
    if ($null -ne $superseded) { $record.tasks += (Get-OttrinTaskRecord 'Ottrin Index' $superseded) }
    $bytes = [Text.Encoding]::UTF8.GetBytes(($record | ConvertTo-Json -Compress))
    $file = [IO.File]::Open($Snapshot, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try { $file.Write($bytes, 0, $bytes.Length); $file.Flush($true) }
    finally { $file.Dispose() }
} elseif ($Phase -eq 'Rollback') {
    # Snapshot can fail before any mutation, in which case there is nothing to undo.
    if (-not (Test-Path -LiteralPath $Snapshot)) { return }
    $record = Read-TransactionSnapshot
    $folder = Task-Folder
    foreach ($taskRecord in $record.tasks) {
        Restore-OttrinTaskRecord $folder $taskRecord
    }
    # Keep the snapshot if restoration failed; never conceal scheduler errors.
    Remove-Item -LiteralPath $Snapshot
} else {
    if ($Phase -eq 'Activate' -and -not (Test-Path -LiteralPath $Snapshot)) {
        throw 'Index activation requires its rollback snapshot.'
    }
    if (Test-Path -LiteralPath $Snapshot) {
        $record = Read-TransactionSnapshot
        $folder = Task-Folder
        $current = Get-OttrinIndexTask $folder
        # Upgrade setup writes the new definition; retain it while restoring
        # preexisting runtime intent. Fresh installs have no prior task. Removal
        # has no current task and must never recreate one during commit.
        $prior = @($record.tasks | Where-Object { $_.name -eq $TaskName -and $null -ne $_.xml })
        if ($prior.Count -eq 0) { $prior = @($record.tasks | Where-Object { $_.name -eq 'Ottrin Index' -and $null -ne $_.xml }) }
        if ($null -ne $current -and $prior.Count -ne 0) {
            Restore-OttrinTaskRunningState $current $prior[0].running
        }
        # NSIS activates before its durable payload decision. Keep rollback
        # intent until that decision exists; MSI still uses the single Commit.
        if ($Phase -eq 'Commit') { Remove-Item -LiteralPath $Snapshot }
    }
}

 } -Phase $phase -Snapshot $snapshot
}
$service=New-Object -ComObject Schedule.Service;$service.Connect();$folder=$service.GetFolder('\')
$name=Get-OttrinIndexTaskName
$results=@()
try {
 if($null -ne (Get-OttrinIndexTask $folder)){throw 'QA task already exists'}
 foreach($running in @($false,$true)) {
  $d=$service.NewTask(0);$d.Principal.UserId=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value;$d.Principal.LogonType=3
  $d.Settings.Enabled=$true;$d.Settings.DisallowStartIfOnBatteries=$false;$d.Settings.StopIfGoingOnBatteries=$false
  $a=$d.Actions.Create(0);$a.Path="$env:WINDIR\System32\WindowsPowerShell\v1.0\powershell.exe";$a.Arguments='-NoProfile -NonInteractive -Command Start-Sleep -Seconds 90'
  $task=$folder.RegisterTaskDefinition($name,$d,6,$null,$null,3,$null)
  if($running){Restore-OttrinTaskRunningState $task $true}
  $snapshot=Join-Path $r ('task-'+$running+'.json')
  Transaction Snapshot $snapshot
  $hash=(Get-FileHash $snapshot).Hash
  Stop-OttrinIndexTask $task
  $d.RegistrationInfo.Description='new definition retained'
  $task=$folder.RegisterTaskDefinition($name,$d,6,$null,$null,3,$null)
  Transaction Activate $snapshot
  if((Get-FileHash $snapshot).Hash -cne $hash){throw 'Activation changed snapshot'}
  if(($task.GetInstances(0).Count -ne 0) -ne $running){throw 'Activation runtime intent mismatch'}
  if($task.Definition.RegistrationInfo.Description -cne 'new definition retained'){throw 'Activation restored old definition'}
  Transaction Commit $snapshot
  if(Test-Path $snapshot){throw 'Commit retained snapshot'}
  $results+=@{prior_running=$running;activation_snapshot_unchanged=$true;new_definition_retained=$true;running_intent_preserved=$true;commit_snapshot_removed=$true}
  Stop-OttrinIndexTask $task;$folder.DeleteTask($name,0)
 }
 $rejected=$false;try{Transaction Activate (Join-Path $r 'absent.json')}catch{$rejected=$true}
 if(-not $rejected){throw 'Missing activation snapshot accepted'}
 @{status='pass';cases=$results;missing_activation_snapshot_refused=$true}|ConvertTo-Json -Depth 5|Set-Content "$r\TASK_RESULTS.json"
} catch {$_|Out-String|Set-Content "$r\task-failure.txt";exit 1}
finally {
 $task=Get-OttrinIndexTask $folder
 if($null -ne $task){Stop-OttrinIndexTask $task;$folder.DeleteTask($name,0)}
}
