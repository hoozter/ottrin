param(
    [Parameter(Mandatory = $true)]
    [string]$Stage,
    [string]$Version = ""
)

$ErrorActionPreference = "Stop"
$Root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$Stage = (Resolve-Path $Stage).Path
$DistDir = if ($env:OTTRIN_DIST_DIR) { $env:OTTRIN_DIST_DIR } else { Join-Path $Root "dist" }

& git -C $Root diff --quiet --ignore-submodules --
if ($LASTEXITCODE -ne 0) { throw "Refusing to package a tracked-dirty checkout; commit or stash tracked changes first." }
& git -C $Root diff --cached --quiet --ignore-submodules --
if ($LASTEXITCODE -ne 0) { throw "Refusing to package a tracked-dirty checkout; commit or stash tracked changes first." }

if (-not $Version) {
    $VersionLine = Get-Content (Join-Path $Root "Cargo.toml") |
        Where-Object { $_ -match '^version = ' } |
        Select-Object -First 1
    $Version = $VersionLine -replace 'version = "([^"]+)"', '$1'
}

$RequiredFiles = @(
    "bin\ottrin.exe",
    "bin\ottrin-indexd.exe",
    "assets\icon.ico",
    "bin\ottrin-priv-helper.exe",
    "bin\pdfium\pdfium.dll",
    "bin\opengl32.dll",
    "bin\libEGL.dll",
    "bin\libGLESv2.dll",
    "bin\libgallium_wgl.dll",
    "bin\dxil.dll",
    "third-party-notices\MANIFEST.txt",
    "SOURCE_REVISION.txt",
    "LICENSE",
    "RUNNING.md",
    "README.md",
    "docs\USER_GUIDE.md",
    "docs\DEVELOPMENT.md",
    "docs\ROADMAP.md",
    "docs\RELEASE_NOTES.md",
    "docs\RELEASE_CHECKLIST.md"
)
foreach ($RelativePath in $RequiredFiles) {
    if (-not (Test-Path (Join-Path $Stage $RelativePath) -PathType Leaf)) {
        throw "MSI payload is incomplete: missing $RelativePath"
    }
}

$ReleaseFiles = @(
    "LICENSE",
    "README.md",
    "docs\USER_GUIDE.md",
    "docs\DEVELOPMENT.md",
    "docs\ROADMAP.md",
    "docs\RELEASE_NOTES.md",
    "docs\RELEASE_CHECKLIST.md"
)
foreach ($RelativePath in $ReleaseFiles) {
    $StageHash = (Get-FileHash -Algorithm SHA256 (Join-Path $Stage $RelativePath)).Hash
    $SourceHash = (Get-FileHash -Algorithm SHA256 (Join-Path $Root $RelativePath)).Hash
    if ($StageHash -ne $SourceHash) { throw "MSI payload is stale: $RelativePath" }
}

$SourceRevision = (& git -C $Root rev-parse --verify HEAD).Trim()
if ($LASTEXITCODE -ne 0) { throw "Could not determine the source revision for MSI packaging." }
$RevisionLines = Get-Content (Join-Path $Stage "SOURCE_REVISION.txt")
if ($RevisionLines -notcontains "Version: $Version") { throw "MSI payload has a stale package version." }
if ($RevisionLines -notcontains "Git revision: $SourceRevision") { throw "MSI payload has a stale source revision." }

$WixCommand = if ($env:WIX) {
    Get-Command $env:WIX -ErrorAction Stop
} else {
    Get-Command wix.exe -ErrorAction SilentlyContinue
}
if (-not $WixCommand) { $WixCommand = Get-Command wix -ErrorAction SilentlyContinue }
if (-not $WixCommand) { throw "WiX Toolset 5 was not found. Install it with: dotnet tool install --global wix --version 5.*" }
$WixPath = $WixCommand.Source

New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
$Msi = Join-Path $DistDir "Ottrin-$Version-windows-x64.msi"
Remove-Item -Force $Msi, "$Msi.sha256" -ErrorAction SilentlyContinue
& $WixPath build "-arch" "x64" "-d" "Version=$Version" "-d" "Stage=$Stage" "-o" $Msi (Join-Path $PSScriptRoot "installer.wxs")
if ($LASTEXITCODE -ne 0 -or -not (Test-Path $Msi -PathType Leaf)) {
    throw "WiX did not produce the expected MSI."
}

Get-FileHash -Algorithm SHA256 $Msi |
    ForEach-Object { "$($_.Hash.ToLowerInvariant())  $(Split-Path -Leaf $Msi)" } |
    Set-Content -Encoding ASCII "$Msi.sha256"
Write-Output $Msi
