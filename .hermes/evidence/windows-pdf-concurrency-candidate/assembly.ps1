param([Parameter(Mandatory = $true)][string]$Disc)
$ErrorActionPreference = 'Stop'; $ProgressPreference = 'SilentlyContinue'
$Root = 'C:\OttrinOwnedMsixAssembly'
if (Test-Path $Root) { throw 'Existing assembly fixture retained.' }
New-Item -ItemType Directory $Root | Out-Null
$Inputs = Get-Content (Join-Path $Disc 'assembly-inputs.json') -Raw | ConvertFrom-Json
foreach ($InputFile in $Inputs.files) {
    if ((Get-FileHash (Join-Path $Disc $InputFile.name)).Hash.ToLowerInvariant() -ne $InputFile.sha256) { throw 'Guest assembly input hash mismatch.' }
}
Expand-Archive (Join-Path $Disc 'mingit.zip') (Join-Path $Root 'git')
Expand-Archive (Join-Path $Disc 'source.zip') (Join-Path $Root 'source')
Expand-Archive (Join-Path $Disc 'portable.zip') (Join-Path $Root 'portable')
$env:PATH = (Join-Path $Root 'git\cmd') + ';' + $env:PATH
$env:MAKEAPPX = 'C:\OttrinOwnedMsixSdk\bin\10.0.26100.0\x64\makeappx.exe'
$env:OTTRIN_DIST_DIR = Join-Path $Root 'output'
$Source = Join-Path $Root 'source'
$Revision = (& git -C $Source rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $Revision -ne $Inputs.source_revision) { throw 'Real Git source revision mismatch.' }
& git -C $Source diff --quiet
if ($LASTEXITCODE -ne 0) { throw 'Extracted source fixture is dirty.' }
$Stages = @(Get-ChildItem (Join-Path $Root 'portable') -Directory)
if ($Stages.Count -ne 1) { throw 'Expected exactly one actual portable payload.' }
$Stage = $Stages[0].FullName
$Script = Join-Path $Source 'packaging\windows\package-msix.ps1'
$Raw = [IO.File]::ReadAllText($Script)
# Invoke the verified local source inline, binding its normal script directory.
# The fixture does not change machine or per-process execution policy.
$Bound = $Raw.Replace('$PSScriptRoot', "'" + (Split-Path $Script) + "'")
$RejectedRevisions = @()
foreach ($Invalid in @(-1, 65536)) {
    $Rejected = $false
    try { & ([ScriptBlock]::Create($Bound)) -Stage $Stage -Development -DevelopmentRevision $Invalid | Out-Null }
    catch {
        if ($_.FullyQualifiedErrorId -notlike 'ParameterArgumentValidationError*') { throw }
        $Rejected = $true
    }
    if (-not $Rejected) { throw 'Invalid development revision was not rejected during parameter binding.' }
    if (Test-Path $env:OTTRIN_DIST_DIR) { throw 'Rejected revision created package output.' }
    $RejectedRevisions += $Invalid
}
$Output = @(& ([ScriptBlock]::Create($Bound)) -Stage $Stage -Development -DevelopmentRevision 40)
$Packages = @(Get-ChildItem $env:OTTRIN_DIST_DIR -Filter '*.msix' -File)
if ($Packages.Count -ne 1) { throw 'Expected exactly one actual development MSIX.' }
$Package = $Packages[0].FullName
$Receipt = Get-Content ($Package + '.provenance.json') -Raw | ConvertFrom-Json
if ($Receipt.source_revision -ne $Revision -or -not $Receipt.development -or $Receipt.signed -or $Receipt.version -ne '0.1.0.40') { throw 'Development package provenance mismatch.' }
& $env:MAKEAPPX unpack /p $Package /d (Join-Path $Root 'unpacked') /o | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Actual Ottrin MSIX unpack failed.' }
[xml]$UnpackedManifest = Get-Content (Join-Path $Root 'unpacked\AppxManifest.xml') -Raw
if ($UnpackedManifest.Package.Identity.Version -ne '0.1.0.40') { throw 'Native package version does not match requested development revision.' }
$Count = 0
foreach ($File in Get-ChildItem $Stage -Recurse -File) {
    $Relative = $File.FullName.Substring($Stage.Length).TrimStart('\')
    if ((Get-FileHash $File.FullName).Hash -ne (Get-FileHash (Join-Path $Root ('unpacked\' + $Relative))).Hash) { throw "Actual payload mismatch: $Relative" }
    $Count++
}
if (Test-Path (Join-Path $env:OTTRIN_DIST_DIR '.msix-stage')) { throw 'Producer left completed staging.' }

# Verify this changed daemon's runtime instead of inheriting old mapped-image
# equality. This noninteractive assembly account is explicitly separate from
# ordinary-user scheduled-task, installer and native UI acceptance.
$DaemonCheck = @{ status='running'; scope='Exact portable daemon in the disposable noninteractive assembly account, private1000-file corpus, content disabled; watcher/query/restart/idle sample. No ordinary-user task, installer lifecycle, GUI, Store or personal-service claim.' }
$Private = Join-Path $Root 'daemon-state'
$Daemon = $null
$OldAppData = $env:APPDATA; $OldLocalData = $env:LOCALAPPDATA
function DaemonCli($Arguments) {
    $Info = [Diagnostics.ProcessStartInfo]::new((Join-Path $Stage 'bin\ottrin.exe'), $Arguments)
    $Info.UseShellExecute=$false; $Info.CreateNoWindow=$true
    $Info.RedirectStandardOutput=$true; $Info.RedirectStandardError=$true
    $Child = [Diagnostics.Process]::Start($Info)
    try {
        $Stdout=$Child.StandardOutput.ReadToEndAsync(); $Stderr=$Child.StandardError.ReadToEndAsync()
        if (-not $Child.WaitForExit(15000)) { throw "Daemon CLI timed out: $Arguments" }
        if (-not $Stdout.Wait(5000) -or -not $Stderr.Wait(5000)) { throw 'Daemon CLI streams incomplete' }
        if ($Child.ExitCode -ne 0) { throw ('Daemon CLI failed: '+$Stderr.Result) }
        return $Stdout.Result
    } finally {
        if (-not $Child.HasExited) { $Child.Kill(); $null=$Child.WaitForExit(10000) }
        $Child.Dispose()
    }
}
function ReadyDaemon {
    $Deadline=[DateTime]::UtcNow.AddSeconds(45)
    do {
        if ($Daemon.HasExited) { throw 'Private daemon exited before ready' }
        $Status=(DaemonCli 'index status --json') | ConvertFrom-Json
        if ($Status.status -eq 'ready' -and $Status.watcher_ready -and $Status.indexed_items -eq 1000) { return $Status }
        Start-Sleep -Milliseconds 200
    } while ([DateTime]::UtcNow -lt $Deadline)
    throw 'Private daemon did not report1000files and live watcher'
}
function AwaitSearch($Text,$Count) {
    $Deadline=[DateTime]::UtcNow.AddSeconds(30)
    do {
        $Response=(DaemonCli ('search '+$Text+' --json')) | ConvertFrom-Json
        if (@($Response.items).Count -eq $Count) { return }
        Start-Sleep -Milliseconds 200
    } while ([DateTime]::UtcNow -lt $Deadline)
    throw ('Filesystem watcher query mismatch: '+$Text)
}
try {
    if (@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue).Count) { throw 'Unexpected daemon retained; do not share another process endpoint' }
    $null=New-Item -ItemType Directory $Private
    $env:APPDATA=Join-Path $Private 'roaming'; $env:LOCALAPPDATA=Join-Path $Private 'local'
    $Corpus=Join-Path $Private 'corpus'; $ConfigDir=Join-Path $env:APPDATA 'ottrin'
    $null=New-Item -ItemType Directory $Corpus,$ConfigDir
    for ($I=0;$I -lt 1000;$I++) { [IO.File]::WriteAllText((Join-Path $Corpus ('original-{0:D4}.txt' -f $I)),('owned original'+$I)) }
    $OriginalHashes=@{}; foreach($File in Get-ChildItem $Corpus -File) { $OriginalHashes[$File.Name]=(Get-FileHash $File.FullName).Hash }
    $Config=Get-Content (Join-Path $Disc 'daemon-config-template.json') -Raw | ConvertFrom-Json
    $Config.search.include_roots=@($Corpus); $Config.search.exclude_roots=@(); $Config.search.indexed_volumes=@()
    $Config.search.content_indexing_enabled=$false; $Config.search.use_system_locate=$false
    $Config.search.manage_locate_db=$false; $Config.search.default_scope='Global'
    $Config.search.include_globs=@(); $Config.search.exclude_globs=@()
    $Pref=Join-Path $ConfigDir 'config.json'
    [IO.File]::WriteAllText($Pref,($Config | ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false))
    $PrefHash=(Get-FileHash $Pref).Hash
    $DaemonPath=Join-Path $Stage 'bin\ottrin-indexd.exe'
    $DaemonCheck.binary_sha256=(Get-FileHash $DaemonPath).Hash.ToLowerInvariant()
    $DaemonCheck.account_sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    $Start=[Diagnostics.ProcessStartInfo]::new($DaemonPath); $Start.UseShellExecute=$false
    $Daemon=[Diagnostics.Process]::Start($Start)
    $DaemonCheck.initial_ready=ReadyDaemon
    AwaitSearch 'original-0999' 1
    $Needle=Join-Path $Corpus 'watchneedle.txt'; [IO.File]::WriteAllText($Needle,'owned mutation')
    AwaitSearch 'watchneedle' 1
    $Renamed=Join-Path $Corpus 'renamedneedle.txt'; Move-Item $Needle $Renamed
    AwaitSearch 'watchneedle' 0; AwaitSearch 'renamedneedle' 1
    Remove-Item $Renamed; AwaitSearch 'renamedneedle' 0
    $null=ReadyDaemon; Start-Sleep -Seconds 2
    $Daemon.Refresh(); $Before=$Daemon.TotalProcessorTime.TotalSeconds
    $Clock=[Diagnostics.Stopwatch]::StartNew(); Start-Sleep -Seconds 30
    $Daemon.Refresh(); $Seconds=$Clock.Elapsed.TotalSeconds
    $Cpu=100*($Daemon.TotalProcessorTime.TotalSeconds-$Before)/$Seconds
    $DaemonCheck.idle=@{seconds=$Seconds;cpu_one_core_percent=$Cpu;criterion_below_one_percent=($Cpu -lt 1);no_clients_during_sample=$true}
    if ($Cpu -ge 1) { throw 'Current private daemon exceeds quiet CPU criterion' }
    $Stop=[Diagnostics.ProcessStartInfo]::new($DaemonPath,'--stop'); $Stop.UseShellExecute=$false
    $Stopping=[Diagnostics.Process]::Start($Stop)
    try { if (-not $Stopping.WaitForExit(15000) -or $Stopping.ExitCode -ne 0) { throw 'Protocol stop failed' } }
    finally { if (-not $Stopping.HasExited) { $Stopping.Kill(); $null=$Stopping.WaitForExit(5000) }; $Stopping.Dispose() }
    if (-not $Daemon.WaitForExit(15000) -or $Daemon.ExitCode -ne 0) { throw 'Private daemon did not exit cleanly' }
    $Daemon.Dispose(); $Daemon=[Diagnostics.Process]::Start($Start)
    $DaemonCheck.restart_ready=ReadyDaemon
    AwaitSearch 'original-0999' 1; AwaitSearch 'watchneedle' 0; AwaitSearch 'renamedneedle' 0
    $Stopping=[Diagnostics.Process]::Start($Stop)
    try { if (-not $Stopping.WaitForExit(15000) -or $Stopping.ExitCode -ne 0) { throw 'Restart protocol stop failed' } }
    finally { if (-not $Stopping.HasExited) { $Stopping.Kill(); $null=$Stopping.WaitForExit(5000) }; $Stopping.Dispose() }
    if (-not $Daemon.WaitForExit(15000) -or $Daemon.ExitCode -ne 0) { throw 'Restart did not exit cleanly' }
    if ((Get-FileHash $Pref).Hash -ne $PrefHash -or @(Get-ChildItem $Corpus -File).Count -ne 1000) { throw 'Private config or original corpus changed' }
    foreach($File in Get-ChildItem $Corpus -File) { if ((Get-FileHash $File.FullName).Hash -ne $OriginalHashes[$File.Name]) { throw 'Original source changed' } }
    $DaemonCheck.status='pass scoped rebuilt daemon runtime and idle sample'
    $DaemonCheck.watcher_mutations=@('create','rename','delete'); $DaemonCheck.protocol_shutdowns=2
    $DaemonCheck.source_and_preferences_preserved=$true
} finally {
    if ($Daemon) { if (-not $Daemon.HasExited) { $Daemon.Kill(); $null=$Daemon.WaitForExit(10000) }; $Daemon.Dispose() }
    $env:APPDATA=$OldAppData; $env:LOCALAPPDATA=$OldLocalData
    if (Test-Path $Private) { Remove-Item $Private -Recurse -Force }
    $DaemonCheck.private_state_removed=(-not(Test-Path $Private))
}
@{
    status = 'pass'; source_revision = $Revision; payload_files = $Count; rejected_revisions = $RejectedRevisions
    source_script_sha256 = (Get-FileHash $Script).Hash; artifact = $Package
    artifact_sha256 = (Get-FileHash $Package).Hash; artifact_bytes = (Get-Item $Package).Length; provenance = $Receipt; daemon_runtime = $DaemonCheck
    scope = 'Actual clean-source production MSIX assembly and exact portable payload comparison; no deployment, launch, index upgrade or uninstall acceptance yet.'
} | ConvertTo-Json -Depth 6 -Compress
