param(
    [Parameter(Mandatory = $true)]
    [string]$Stage,
    [string]$Version = ""
)

$ErrorActionPreference = "Stop"
$Root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$Stage = (Resolve-Path $Stage).Path
$DistDir = Join-Path $Root "dist"

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
    "README.md",
    "docs\USER_GUIDE.md",
    "docs\RELEASE_NOTES.md",
    "docs\RELEASE_CHECKLIST.md"
)
foreach ($RelativePath in $RequiredFiles) {
    if (-not (Test-Path (Join-Path $Stage $RelativePath))) {
        throw "Installer payload is incomplete: missing $RelativePath"
    }
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
