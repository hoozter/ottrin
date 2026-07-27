param(
    [Parameter(Mandatory = $true)]
    [string]$Stage,
    [string]$Version = ""
)

$ErrorActionPreference = "Stop"
$Root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$Stage = (Resolve-Path $Stage).Path
$DistDir = if ($env:OTTRIN_DIST_DIR) { $env:OTTRIN_DIST_DIR } else { Join-Path $Root "dist" }

if (-not $Version) {
    $CargoToml = Get-Content (Join-Path $Root "Cargo.toml")
    $VersionLine = $CargoToml | Where-Object { $_ -match '^version = ' } | Select-Object -First 1
    $Version = $VersionLine -replace 'version = "([^"]+)"', '$1'
}

$RequiredFiles = @(
    "bin\ottrin.exe",
    "bin\ottrin-index.exe",
    "bin\ottrin-priv-helper.exe",
    "bin\pdfium\pdfium.dll",
    "bin\pdfium\LICENSE",
    "bin\pdfium\licenses\pdfium.txt",
    "bin\libEGL.dll",
    "bin\libGLESv2.dll",
    "bin\libgallium_wgl.dll",
    "bin\dxil.dll",
    "third-party-notices\MANIFEST.txt",
    "third-party-notices\mesa3d-26.1.3\docs\license.rst",
    "third-party-notices\mesa3d-26.1.3\licenses\Apache-2.0",
    "third-party-notices\mesa3d-26.1.3\licenses\BSL-1.0",
    "third-party-notices\mesa3d-26.1.3\licenses\GPL-1.0-or-later",
    "third-party-notices\mesa3d-26.1.3\licenses\GPL-2.0-only",
    "third-party-notices\mesa3d-26.1.3\licenses\MIT",
    "third-party-notices\mesa3d-26.1.3\licenses\SGI-B-2.0",
    "third-party-notices\mesa3d-26.1.3\licenses\exceptions\Linux-Syscall-Note",
    "third-party-notices\pal1000-LICENSE.txt",
    "third-party-notices\Microsoft-DXC-LICENSE-MS.txt",
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
    if (-not (Test-Path (Join-Path $Stage $RelativePath))) {
        throw "Installer payload is incomplete: missing $RelativePath"
    }
}

# NSIS copies this stage verbatim. Keep release-facing files tied to this
# checkout instead of accepting a stale portable stage with the right names.
$ReleaseFiles = @(
    "LICENSE",
    "README.md",
    "docs\\USER_GUIDE.md",
    "docs\\DEVELOPMENT.md",
    "docs\\ROADMAP.md",
    "docs\\RELEASE_NOTES.md",
    "docs\\RELEASE_CHECKLIST.md"
)
foreach ($RelativePath in $ReleaseFiles) {
    $StageHash = (Get-FileHash -Algorithm SHA256 (Join-Path $Stage $RelativePath)).Hash
    $SourceHash = (Get-FileHash -Algorithm SHA256 (Join-Path $Root $RelativePath)).Hash
    if ($StageHash -ne $SourceHash) {
        throw "Installer payload is stale: $RelativePath"
    }
}

$SourceRevision = (& git -C $Root rev-parse --verify HEAD).Trim()
if ($LASTEXITCODE -ne 0) { throw "Could not determine the source revision for installer packaging." }
if (-not (Select-String -Path (Join-Path $Stage "SOURCE_REVISION.txt") -SimpleMatch "Git revision: $SourceRevision" -Quiet)) {
    throw "Installer payload has a stale source revision."
}

$MakensisCommand = Get-Command makensis.exe -ErrorAction SilentlyContinue
if ($MakensisCommand) {
    $MakensisPath = $MakensisCommand.Source
} else {
    $DefaultMakensis = "${env:ProgramFiles(x86)}\NSIS\makensis.exe"
    if (Test-Path $DefaultMakensis) {
        $MakensisPath = $DefaultMakensis
    } else {
        throw "makensis.exe was not found. Install NSIS before packaging the installer."
    }
}

New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
$Installer = Join-Path $DistDir "Ottrin-$Version-windows-x64-setup.exe"
Remove-Item -Force $Installer -ErrorAction SilentlyContinue

& $MakensisPath "/DVERSION=$Version" "/DSTAGE=$Stage" "/DOUTPUT=$Installer" (Join-Path $PSScriptRoot "installer.nsi")
if ($LASTEXITCODE -ne 0 -or -not (Test-Path $Installer)) {
    throw "NSIS did not produce the expected installer."
}

Get-FileHash -Algorithm SHA256 $Installer |
    ForEach-Object { "$($_.Hash.ToLowerInvariant())  $(Split-Path -Leaf $Installer)" } |
    Set-Content -Encoding ASCII "$Installer.sha256"

Write-Output $Installer
