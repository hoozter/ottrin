$ErrorActionPreference = "Stop"

$Root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$CargoToml = Get-Content (Join-Path $Root "Cargo.toml")
$VersionLine = $CargoToml | Where-Object { $_ -match '^version = ' } | Select-Object -First 1
$Version = if ($env:OTTRIN_VERSION) { $env:OTTRIN_VERSION } else { ($VersionLine -replace 'version = "([^"]+)"', '$1') }
$Target = if ($env:OTTRIN_TARGET) { $env:OTTRIN_TARGET } else { "x86_64-pc-windows-gnu" }
$BuildDir = Join-Path $Root "target\$Target\release"
$DistDir = if ($env:OTTRIN_DIST_DIR) { $env:OTTRIN_DIST_DIR } else { Join-Path $Root "dist" }
$Stage = Join-Path $DistDir "Ottrin-$Version-windows-x64"
$Archive = Join-Path $DistDir "Ottrin-$Version-windows-x64-portable.zip"
$PdfiumUrl = "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/7961/pdfium-win-x64.tgz"
$PdfiumSha256 = "88276459349b291c41f10422dad0210f007c04d919c8fa56472b6b7c6406adf4"
$MesaUrl = "https://github.com/pal1000/mesa-dist-win/releases/download/26.1.3/mesa3d-26.1.3-release-mingw.7z"
$MesaSha256 = "80d5add64254c839b4c784bdab6a2b504e448675604b0fe54a9bce3c543303a7"
$MesaSourceUrl = "https://gitlab.freedesktop.org/mesa/mesa/-/archive/mesa-26.1.3/mesa-mesa-26.1.3.tar.gz"
$MesaSourceSha256 = "4ef7c87feb15966d5645ec55728020632d2f54becb42ce546d4a12bfc5275dd1"
$MesaLibEglSha256 = "ce36d60c5602920df49a2c8939d903da05dfe2610bfbb4bf0676e9ec3453df3b"
$MesaLibGlesv2Sha256 = "649f779b1b945433dfc2740e9ff1366e309686043999f7271e550079eadde8e6"
$MesaLibGalliumWglSha256 = "a3917a52cc893f614c9e415f5442eb2981dd46ba636fc64b35d5bd4be0faec7b"
$PalLicenseUrl = "https://raw.githubusercontent.com/pal1000/mesa-dist-win/26.1.3/LICENSE"
$PalLicenseSha256 = "9cd0121dc070f0ac65c1fc266bee74bdbbc22cea4210ae72ae0c1f076448d4cc"
$DxcUrl = "https://github.com/microsoft/DirectXShaderCompiler/releases/download/v1.9.2602.24/dxc_2026_05_27.zip"
$DxcSha256 = "cf658aacf070d3045e31b8f1f8a696c2945f37c1095019481ef7c513368db3b4"
$DxilSha256 = "cbcfe883a09fd0ca1f98abdf3a9553b560895e3283a136da82a8381253a169df"

function Get-VerifiedFile {
    param([string]$Url, [string]$ExpectedSha256, [string]$Destination)
    if (Get-Command curl.exe -ErrorAction SilentlyContinue) {
        & curl.exe -fsSL $Url -o $Destination
    } else {
        Invoke-WebRequest -Uri $Url -OutFile $Destination
    }
    $actual = (Get-FileHash -Algorithm SHA256 $Destination).Hash.ToLowerInvariant()
    if ($actual -ne $ExpectedSha256.ToLowerInvariant()) {
        throw "Checksum mismatch for $Url: expected $ExpectedSha256, got $actual"
    }
}

function Resolve-PdfiumRuntime {
    param([string]$Destination)
    $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("ottrin-pdfium-" + [guid]::NewGuid())
    New-Item -ItemType Directory -Force -Path $tmp | Out-Null
    try {
        $archive = Join-Path $tmp "pdfium.tgz"
        Get-VerifiedFile $PdfiumUrl $PdfiumSha256 $archive
        $extract = Join-Path $tmp "extract"
        New-Item -ItemType Directory -Force -Path $extract | Out-Null
        & tar -xzf $archive -C $extract
        if ($LASTEXITCODE -ne 0) { throw "PDFium extraction from the pinned archive failed." }

        New-Item -ItemType Directory -Force -Path $Destination | Out-Null
        Copy-Item (Join-Path $extract "bin\*") $Destination -Recurse
        Copy-Item (Join-Path $extract "LICENSE") $Destination
        Copy-Item (Join-Path $extract "licenses") $Destination -Recurse
    } finally {
        Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    }
}

function Resolve-GraphicsCompatibilityRuntime {
    param([string]$BinDestination, [string]$NoticeDestination)
    $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("ottrin-mesa-" + [guid]::NewGuid())
    New-Item -ItemType Directory -Force -Path $tmp, $NoticeDestination | Out-Null
    try {
    $mesaArchive = Join-Path $tmp "mesa.7z"
    $mesaSource = Join-Path $tmp "mesa-source.tar.gz"
    $dxcArchive = Join-Path $tmp "dxc.zip"
    Get-VerifiedFile $MesaUrl $MesaSha256 $mesaArchive
    Get-VerifiedFile $MesaSourceUrl $MesaSourceSha256 $mesaSource
    Get-VerifiedFile $PalLicenseUrl $PalLicenseSha256 (Join-Path $tmp "pal1000-LICENSE.txt")
    Get-VerifiedFile $DxcUrl $DxcSha256 $dxcArchive
    $sevenZip = Get-Command 7z.exe -ErrorAction SilentlyContinue
    if (-not $sevenZip) { throw "7z.exe is required to extract the pinned Mesa graphics compatibility archive." }
    $mesaDllChecksums = @{
        "libEGL.dll" = $MesaLibEglSha256
        "libGLESv2.dll" = $MesaLibGlesv2Sha256
        "libgallium_wgl.dll" = $MesaLibGalliumWglSha256
    }
    foreach ($dll in $mesaDllChecksums.Keys) {
        & $sevenZip.Source e "-o$BinDestination" -y $mesaArchive "x64/$dll" | Out-Null
        $actual = (Get-FileHash -Algorithm SHA256 (Join-Path $BinDestination $dll)).Hash.ToLowerInvariant()
        if ($actual -ne $mesaDllChecksums[$dll].ToLowerInvariant()) {
            throw "Mesa DLL checksum mismatch after extracting the pinned Mesa archive: $dll"
        }
    }
    Expand-Archive -Path $dxcArchive -DestinationPath (Join-Path $tmp "dxc")
    $dxil = Join-Path $tmp "dxc\bin\x64\dxil.dll"
    if ((Get-FileHash -Algorithm SHA256 $dxil).Hash.ToLowerInvariant() -ne $DxilSha256) {
        throw "DXIL checksum mismatch after extracting the pinned Microsoft DXC archive."
    }
    Copy-Item $dxil $BinDestination
    & tar -xzf $mesaSource -C $NoticeDestination --strip-components=1 mesa-mesa-26.1.3/docs/license.rst mesa-mesa-26.1.3/licenses
    if ($LASTEXITCODE -ne 0) {
        throw "Mesa license extraction from the pinned source archive failed."
    }
    Copy-Item (Join-Path $tmp "pal1000-LICENSE.txt") (Join-Path $NoticeDestination "pal1000-LICENSE.txt")
    Copy-Item (Join-Path $tmp "dxc\LICENSE-MS.txt") (Join-Path $NoticeDestination "Microsoft-DXC-LICENSE-MS.txt")
    @"
Ottrin Windows graphics compatibility runtime

Mesa archive: $MesaUrl
Mesa archive SHA-256: $MesaSha256
Mesa source archive: $MesaSourceUrl
Mesa source archive SHA-256: $MesaSourceSha256
libEGL.dll SHA-256: $MesaLibEglSha256
libGLESv2.dll SHA-256: $MesaLibGlesv2Sha256
libgallium_wgl.dll SHA-256: $MesaLibGalliumWglSha256
pal1000 license: $PalLicenseUrl
pal1000 license SHA-256: $PalLicenseSha256
Microsoft DXC archive: $DxcUrl
Microsoft DXC archive SHA-256: $DxcSha256
dxil.dll SHA-256: $DxilSha256
Included Mesa DLLs: libEGL.dll, libGLESv2.dll, libgallium_wgl.dll
"@ | Set-Content -Encoding UTF8 (Join-Path $NoticeDestination "MANIFEST.txt")
    } finally {
        Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    }
}

Set-Location $Root
cargo build --release --target $Target -p ottrin-app -p ottrin-platform -p ottrin-search

Remove-Item -Recurse -Force $Stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path "$Stage\bin", "$Stage\assets", "$Stage\docs", "$Stage\third-party-notices" | Out-Null

Copy-Item "$BuildDir\ottrin-app.exe" "$Stage\bin\ottrin.exe"
Copy-Item "$BuildDir\ottrin-priv-helper.exe" "$Stage\bin\ottrin-priv-helper.exe"
Copy-Item "$BuildDir\ottrin-index.exe" "$Stage\bin\ottrin-index.exe"
Resolve-PdfiumRuntime (Join-Path $Stage "bin\pdfium")
Resolve-GraphicsCompatibilityRuntime (Join-Path $Stage "bin") (Join-Path $Stage "third-party-notices")
Copy-Item "$Root\assets\icon.svg", "$Root\assets\icon.png", "$Root\assets\logo.svg" "$Stage\assets\"
Copy-Item "$Root\README.md" "$Stage\"
if (Test-Path "$Root\LICENSE") { Copy-Item "$Root\LICENSE" "$Stage\" }
Copy-Item "$Root\docs\USER_GUIDE.md", "$Root\docs\DEVELOPMENT.md", "$Root\docs\ROADMAP.md", "$Root\docs\RELEASE_NOTES.md", "$Root\docs\RELEASE_CHECKLIST.md" "$Stage\docs\"

@"
# Running Ottrin Portable on Windows

Run:

```powershell
.\bin\ottrin.exe
```

The privileged helper is bundled next to the app as:

```text
.\bin\ottrin-priv-helper.exe
```

Normal user-owned file operations do not require elevation. Denied operations use the bundled helper through the Windows UAC flow.

On virtual, remote, or problematic graphics adapters, keep the bundled Mesa
compatibility runtime and explicitly select it for this launch:

```powershell
.\bin\ottrin.exe --renderer opengl
```

For a user-level recovery override, set `OTTRIN_RENDERER=opengl` before launching
Ottrin. The normal default remains WGPU with the system graphics driver.
"@ | Set-Content -Encoding UTF8 "$Stage\RUNNING.md"

Remove-Item -Force $Archive -ErrorAction SilentlyContinue
Compress-Archive -Path $Stage -DestinationPath $Archive
Get-FileHash -Algorithm SHA256 $Archive |
    ForEach-Object { "$($_.Hash.ToLowerInvariant())  $(Split-Path -Leaf $Archive)" } |
    Set-Content -Encoding ASCII "$Archive.sha256"

Write-Output $Archive
