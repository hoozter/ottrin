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
$Output = @(& ([ScriptBlock]::Create($Bound)) -Stage $Stage -Development -DevelopmentRevision 6)
$Packages = @(Get-ChildItem $env:OTTRIN_DIST_DIR -Filter '*.msix' -File)
if ($Packages.Count -ne 1) { throw 'Expected exactly one actual development MSIX.' }
$Package = $Packages[0].FullName
$Receipt = Get-Content ($Package + '.provenance.json') -Raw | ConvertFrom-Json
if ($Receipt.source_revision -ne $Revision -or -not $Receipt.development -or $Receipt.signed -or $Receipt.version -ne '0.1.0.6') { throw 'Development package provenance mismatch.' }
& $env:MAKEAPPX unpack /p $Package /d (Join-Path $Root 'unpacked') /o | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Actual Ottrin MSIX unpack failed.' }
[xml]$UnpackedManifest = Get-Content (Join-Path $Root 'unpacked\AppxManifest.xml') -Raw
if ($UnpackedManifest.Package.Identity.Version -ne '0.1.0.6') { throw 'Native package version does not match requested development revision.' }
$Count = 0
foreach ($File in Get-ChildItem $Stage -Recurse -File) {
    $Relative = $File.FullName.Substring($Stage.Length).TrimStart('\')
    if ((Get-FileHash $File.FullName).Hash -ne (Get-FileHash (Join-Path $Root ('unpacked\' + $Relative))).Hash) { throw "Actual payload mismatch: $Relative" }
    $Count++
}
if (Test-Path (Join-Path $env:OTTRIN_DIST_DIR '.msix-stage')) { throw 'Producer left completed staging.' }
@{
    status = 'pass'; source_revision = $Revision; payload_files = $Count; rejected_revisions = $RejectedRevisions
    source_script_sha256 = (Get-FileHash $Script).Hash; artifact = $Package
    artifact_sha256 = (Get-FileHash $Package).Hash; artifact_bytes = (Get-Item $Package).Length; provenance = $Receipt
    scope = 'Actual clean-source production MSIX assembly and exact portable payload comparison; no deployment, launch, index upgrade or uninstall acceptance yet.'
} | ConvertTo-Json -Depth 6 -Compress
