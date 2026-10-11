param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('Snapshot', 'Rollback', 'Commit')]
    [string]$Phase,
    [Parameter(Mandatory = $true)]
    [string]$Snapshot
)

# MSI owns this short-lived transaction; the application owns normal task setup.
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$TaskName = 'Ottrin Index'

function Task-Folder {
    $service = New-Object -ComObject Schedule.Service
    $service.Connect()
    $service.GetFolder('\')
}

function Existing-Task($folder) {
    try { return $folder.GetTask($TaskName) }
    catch {
        if ($_.Exception.GetBaseException().HResult -eq -2147024894) { return $null }
        throw
    }
}

if ($Phase -eq 'Snapshot') {
    if (Test-Path -LiteralPath $Snapshot) {
        # A failed restoration remains available for diagnosis/recovery. Do not
        # let this transaction's rollback consume an earlier transaction's data.
        $retained = "$Snapshot.failed-$([Guid]::NewGuid().ToString('N'))"
        Move-Item -LiteralPath $Snapshot -Destination $retained
        throw "A previous index rollback snapshot requires attention: $retained"
    }
    $task = Existing-Task (Task-Folder)
    $record = @{ version = 1; user = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value; xml = $null; security = $null }
    if ($null -ne $task) {
        $record.xml = $task.Xml
        $record.security = $task.GetSecurityDescriptor(7)
        # Ottrin uses the current interactive user, with no stored credentials.
        if ($task.Definition.Principal.LogonType -ne 3) {
            throw 'The existing index task uses an unsupported logon type; setup was not changed.'
        }
        $principal = $task.Definition.Principal.UserId
        $owner = if ($principal.StartsWith('S-1-')) {
            [Security.Principal.SecurityIdentifier]::new($principal)
        } else {
            ([Security.Principal.NTAccount]::new($principal)).Translate([Security.Principal.SecurityIdentifier])
        }
        if ($owner.Value -ne $record.user) {
            throw 'The existing index task belongs to another user; setup was not changed.'
        }
    }
    $bytes = [Text.Encoding]::UTF8.GetBytes(($record | ConvertTo-Json -Compress))
    $file = [IO.File]::Open($Snapshot, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try { $file.Write($bytes, 0, $bytes.Length); $file.Flush($true) }
    finally { $file.Dispose() }
} elseif ($Phase -eq 'Rollback') {
    # Snapshot can fail before any mutation, in which case there is nothing to undo.
    if (-not (Test-Path -LiteralPath $Snapshot)) { return }
    $record = Get-Content -LiteralPath $Snapshot -Encoding UTF8 -Raw | ConvertFrom-Json
    if ($record.version -ne 1 -or $record.user -ne [Security.Principal.WindowsIdentity]::GetCurrent().User.Value) {
        throw 'Index rollback snapshot does not belong to this user or schema.'
    }
    $folder = Task-Folder
    if ($null -eq $record.xml) {
        if ($null -ne (Existing-Task $folder)) { $folder.DeleteTask($TaskName, 0) }
    } else {
        # XML retains triggers, enabled state, principal, actions and settings.
        # Avoid an added principal ACE, injected XML security metadata and
        # registration-trigger execution while restoring the prior definition.
        $restored = $folder.RegisterTask($TaskName, $record.xml, 54, $null, $null, 3, $null)
        $restored.SetSecurityDescriptor($record.security, 16)
    }
    # Keep the snapshot if restoration failed; never conceal scheduler errors.
    Remove-Item -LiteralPath $Snapshot
} else {
    if (Test-Path -LiteralPath $Snapshot) { Remove-Item -LiteralPath $Snapshot }
}
