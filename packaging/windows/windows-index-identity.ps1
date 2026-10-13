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
    @{ name = $name; xml = $(if ($null -ne $task) { $task.Xml } else { $null }); security = $(if ($null -ne $task) { $task.GetSecurityDescriptor(7) } else { $null }) }
}

function Restore-OttrinTaskRecord($folder, $record) {
    $existing = $null
    try { $existing = $folder.GetTask($record.name) }
    catch {
        if ($_.Exception.GetBaseException().HResult -ne -2147024894) { throw }
    }
    if ($null -ne $existing) { Assert-OttrinTaskOwner $existing }
    if ($null -eq $record.xml) {
        if ($null -ne $existing) { $folder.DeleteTask($record.name, 0) }
    } else {
        $restored = $folder.RegisterTask($record.name, $record.xml, 54, $null, $null, 3, $null)
        $restored.SetSecurityDescriptor($record.security, 16)
    }
}
