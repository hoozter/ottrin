$ErrorActionPreference = "Stop"

$Root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$CargoToml = Get-Content (Join-Path $Root "Cargo.toml")
$VersionLine = $CargoToml | Where-Object { $_ -match '^version = ' } | Select-Object -First 1
$Version = if ($env:OTTRIN_VERSION) { $env:OTTRIN_VERSION } else { ($VersionLine -replace 'version = "([^"]+)"', '$1') }
$Target = if ($env:OTTRIN_TARGET) { $env:OTTRIN_TARGET } else { "x86_64-pc-windows-gnu" }
$BuildDir = Join-Path $Root "target\$Target\release"
$DistDir = Join-Path $Root "dist"
$Stage = Join-Path $DistDir "Ottrin-$Version-windows-x64"
$Archive = Join-Path $DistDir "Ottrin-$Version-windows-x64-portable.zip"

Set-Location $Root
cargo build --release --target $Target -p ottrin-app -p ottrin-platform -p ottrin-search

Remove-Item -Recurse -Force $Stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path "$Stage\bin", "$Stage\assets", "$Stage\docs" | Out-Null

Copy-Item "$BuildDir\ottrin-app.exe" "$Stage\bin\ottrin.exe"
Copy-Item "$BuildDir\ottrin-priv-helper.exe" "$Stage\bin\ottrin-priv-helper.exe"
Copy-Item "$BuildDir\ottrin-index.exe" "$Stage\bin\ottrin-index.exe"
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
"@ | Set-Content -Encoding UTF8 "$Stage\RUNNING.md"

Remove-Item -Force $Archive -ErrorAction SilentlyContinue
Compress-Archive -Path $Stage -DestinationPath $Archive
Get-FileHash -Algorithm SHA256 $Archive |
    ForEach-Object { "$($_.Hash.ToLowerInvariant())  $(Split-Path -Leaf $Archive)" } |
    Set-Content -Encoding ASCII "$Archive.sha256"

Write-Output $Archive
