param(
    [Parameter(Mandatory = $true)][string]$Stage,
    [Parameter(Mandatory = $true, ParameterSetName = 'Development')][switch]$Development,
    [Parameter(Mandatory = $true, ParameterSetName = 'Store')][string]$PackageName,
    [Parameter(Mandatory = $true, ParameterSetName = 'Store')][string]$Publisher,
    [Parameter(Mandatory = $true, ParameterSetName = 'Store')][string]$PublisherDisplayName
)

$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$Stage = (Resolve-Path $Stage).Path
$Dist = if ($env:OTTRIN_DIST_DIR) { [IO.Path]::GetFullPath($env:OTTRIN_DIST_DIR) } else { Join-Path $Root 'dist' }
& git -C $Root diff --quiet --ignore-submodules --
if ($LASTEXITCODE -ne 0) { throw 'Refusing to package a tracked-dirty checkout.' }
& git -C $Root diff --cached --quiet --ignore-submodules --
if ($LASTEXITCODE -ne 0) { throw 'Refusing to package staged source changes.' }
$Revision = (& git -C $Root rev-parse --verify HEAD).Trim()
if ($LASTEXITCODE -ne 0) { throw 'Cannot determine source revision.' }
$VersionLine = Get-Content (Join-Path $Root 'Cargo.toml') | Where-Object { $_ -match '^version = ' } | Select-Object -First 1
$Version = $VersionLine -replace 'version = "([^"]+)"', '$1'
if ($Version -notmatch '^([0-9]+)\.([0-9]+)\.([0-9]+)$') { throw 'MSIX requires a three-component numeric release version.' }
$Parts = @($Matches[1], $Matches[2], $Matches[3])
if (@($Parts | Where-Object { [int]$_ -gt 65535 }).Count) { throw 'MSIX version component exceeds 65535.' }
$PackageVersion = $Version + '.0'
$Recorded = Get-Content (Join-Path $Stage 'SOURCE_REVISION.txt')
if ($Recorded -notcontains "Version: $Version" -or $Recorded -notcontains "Git revision: $Revision") { throw 'Portable stage does not match this clean source revision/version.' }
if (@(Get-ChildItem $Stage -Force -Recurse | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }).Count) { throw 'MSIX stage must contain ordinary owned files, not links or junctions.' }
foreach ($Relative in @('bin\ottrin.exe', 'bin\ottrin-indexd.exe', 'bin\ottrin-priv-helper.exe', 'bin\rg.exe', 'assets\icon.png', 'third-party-notices\MANIFEST.txt')) {
    if (-not (Test-Path (Join-Path $Stage $Relative) -PathType Leaf)) { throw "Incomplete portable stage: $Relative" }
}
foreach ($Relative in @('LICENSE', 'README.md', 'docs\USER_GUIDE.md', 'docs\DEVELOPMENT.md', 'docs\ROADMAP.md', 'docs\RELEASE_NOTES.md', 'docs\RELEASE_CHECKLIST.md', 'assets\icon.png')) {
    if ((Get-FileHash (Join-Path $Stage $Relative)).Hash -ne (Get-FileHash (Join-Path $Root $Relative)).Hash) { throw "Stale portable stage: $Relative" }
}
if ($Development) {
    # Microsoft's unsigned namespace is deliberately distinct from a Store identity.
    $PackageName = 'Ottrin.Development'
    $Publisher = 'CN=Ottrin Development, OID.2.25.311729368913984317654407730594956997722=1'
    $PublisherDisplayName = 'Ottrin Development'
} elseif ($Publisher -match 'OID\.2\.25\.311729368913984317654407730594956997722') {
    throw 'The development publisher namespace cannot be used for a Store package.'
}
if ($PackageName -notmatch '^[A-Za-z0-9][A-Za-z0-9.-]{2,49}$') { throw 'Invalid MSIX package identity name.' }
$MakeAppx = if ($env:MAKEAPPX) { Get-Command $env:MAKEAPPX -ErrorAction Stop } else { Get-Command MakeAppx.exe -ErrorAction SilentlyContinue }
if (-not $MakeAppx) { throw 'Windows SDK MakeAppx.exe is required; set MAKEAPPX to its exact path.' }
New-Item -ItemType Directory -Force $Dist | Out-Null
$Work = Join-Path $Dist '.msix-stage'
if ($Work.StartsWith($Stage.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'MSIX work directory cannot be inside the source payload.' }
if (Test-Path $Work) { throw 'An existing MSIX work directory must be inspected by its owner before reuse.' }
$Suffix = if ($Development) { '-development' } else { '' }
$Destination = Join-Path $Dist "Ottrin-$Version-windows-x64$Suffix.msix"
$Allocated = $false
try {
    New-Item -ItemType Directory $Work | Out-Null
    $Allocated = $true
    $Payload = Join-Path $Work 'payload'
    Copy-Item $Stage $Payload -Recurse
    $Images = Join-Path $Payload 'msix-assets'
    New-Item -ItemType Directory $Images | Out-Null
    Add-Type -AssemblyName System.Drawing
    $Icon = [Drawing.Image]::FromFile((Join-Path $Stage 'assets\icon.png'))
    try {
        foreach ($Size in @(44, 50, 150)) {
            $Bitmap = [Drawing.Bitmap]::new($Size, $Size)
            try {
                $Graphics = [Drawing.Graphics]::FromImage($Bitmap)
                try {
                    $Graphics.Clear([Drawing.Color]::Transparent)
                    $Graphics.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
                    $Graphics.DrawImage($Icon, 0, 0, $Size, $Size)
                } finally { $Graphics.Dispose() }
                $Bitmap.Save((Join-Path $Images "icon-$Size.png"), [Drawing.Imaging.ImageFormat]::Png)
            } finally { $Bitmap.Dispose() }
        }
    } finally { $Icon.Dispose() }
    [xml]$Manifest = Get-Content (Join-Path $PSScriptRoot 'msix-manifest.xml') -Raw
    $Manifest.Package.Identity.SetAttribute('Name', $PackageName)
    $Manifest.Package.Identity.SetAttribute('Publisher', $Publisher)
    $Manifest.Package.Identity.SetAttribute('Version', $PackageVersion)
    $Manifest.Package.Properties.PublisherDisplayName = $PublisherDisplayName
    if ($Development) { $Manifest.Package.Dependencies.TargetDeviceFamily.SetAttribute('MinVersion', '10.0.22000.0') }
    $Manifest.Save((Join-Path $Payload 'AppxManifest.xml'))
    $Candidate = Join-Path $Work 'Ottrin.msix'
    & $MakeAppx.Source pack /d $Payload /p $Candidate /o
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path $Candidate -PathType Leaf)) { throw 'MakeAppx validation/packaging failed; previous deliverable retained.' }
    $Hash = (Get-FileHash $Candidate).Hash.ToLowerInvariant()
    $Receipt = @{
        artifact = Split-Path $Destination -Leaf; sha256 = $Hash; source_revision = $Revision
        package_name = $PackageName; publisher = $Publisher; version = $PackageVersion
        development = [bool]$Development; signed = $false
        scope = 'MakeAppx-validated package assembly only; deployment, runtime, index upgrade/removal and Store certification remain separate gates.'
    }
    Move-Item $Candidate $Destination -Force
    "$Hash  $(Split-Path $Destination -Leaf)" | Set-Content -Encoding ASCII "$Destination.sha256"
    $Receipt | ConvertTo-Json -Depth 4 | Set-Content -Encoding UTF8 "$Destination.provenance.json"
    Write-Output $Destination
} finally {
    if ($Allocated) { Remove-Item $Work -Recurse -Force }
}
