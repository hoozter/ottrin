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
