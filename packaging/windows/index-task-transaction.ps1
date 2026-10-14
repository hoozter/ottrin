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
. (Join-Path $PSScriptRoot 'windows-index-identity.ps1')
$TaskName = Get-OttrinIndexTaskName
$CommitDirectory = "$Snapshot.scripts"

function Task-Folder {
    $service = New-Object -ComObject Schedule.Service
    $service.Connect()
    $service.GetFolder('\')
}

if ($Phase -eq 'Snapshot') {
    if (Test-Path -LiteralPath $Snapshot) {
        # A failed restoration remains available for diagnosis/recovery. Do not
        # let this transaction's rollback consume an earlier transaction's data.
        $retained = "$Snapshot.failed-$([Guid]::NewGuid().ToString('N'))"
        Move-Item -LiteralPath $Snapshot -Destination $retained
        throw "A previous index rollback snapshot requires attention: $retained"
    }
    # Commit runs after RemoveFiles on uninstall, so it cannot depend on the
    # installed payload. Keep one implementation in a transaction-owned copy.
    if (Test-Path -LiteralPath $CommitDirectory) {
        throw "A previous index transaction script directory requires attention: $CommitDirectory"
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
    New-Item -Path $CommitDirectory -ItemType Directory | Out-Null
    try {
        Copy-Item -LiteralPath $PSCommandPath -Destination (Join-Path $CommitDirectory 'index-task-transaction.ps1')
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'windows-index-identity.ps1') -Destination $CommitDirectory
    } catch {
        Remove-Item -LiteralPath $CommitDirectory -Recurse
        throw
    }
} elseif ($Phase -eq 'Rollback') {
    # Snapshot can fail before any mutation, in which case there is nothing to undo.
    if (-not (Test-Path -LiteralPath $Snapshot)) { return }
    $record = Get-Content -LiteralPath $Snapshot -Encoding UTF8 -Raw | ConvertFrom-Json
    if ($record.version -ne 3 -or $record.user -ne [Security.Principal.WindowsIdentity]::GetCurrent().User.Value) {
        throw 'Index rollback snapshot does not belong to this user or schema.'
    }
    $folder = Task-Folder
    foreach ($taskRecord in $record.tasks) {
        if ($taskRecord.name -notin @($TaskName, 'Ottrin Index')) { throw 'Rollback task identity is outside this transaction.' }
        Restore-OttrinTaskRecord $folder $taskRecord
    }
    # Keep the snapshot if restoration failed; never conceal scheduler errors.
    Remove-Item -LiteralPath $Snapshot
    if (Test-Path -LiteralPath $CommitDirectory) { Remove-Item -LiteralPath $CommitDirectory -Recurse }
} else {
    if (Test-Path -LiteralPath $Snapshot) { Remove-Item -LiteralPath $Snapshot }
    if (Test-Path -LiteralPath $CommitDirectory) { Remove-Item -LiteralPath $CommitDirectory -Recurse }
}
