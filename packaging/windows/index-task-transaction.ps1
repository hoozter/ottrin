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
