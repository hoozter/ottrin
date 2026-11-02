param(
    [Parameter(Mandatory = $true)][string]$MakeAppx,
    [Parameter(Mandatory = $true)][string]$Template,
    [Parameter(Mandatory = $true)][string]$Icon
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$Work = Join-Path $env:TEMP ('ottrin-msix-template-' + [Guid]::NewGuid().ToString('N'))
try {
    New-Item -ItemType Directory (Join-Path $Work 'payload\bin'), (Join-Path $Work 'payload\msix-assets') | Out-Null
    # SDK/schema control only: this Windows command is never presented as Ottrin.
    $Source = Join-Path $env:SystemRoot 'System32\cmd.exe'
    Copy-Item $Source (Join-Path $Work 'payload\bin\ottrin.exe')
    Add-Type -AssemblyName System.Drawing
    $Image = [Drawing.Image]::FromFile($Icon)
    try {
        foreach ($Size in @(44, 50, 150)) {
            $Bitmap = [Drawing.Bitmap]::new($Image, $Size, $Size)
            try { $Bitmap.Save((Join-Path $Work "payload\msix-assets\icon-$Size.png"), [Drawing.Imaging.ImageFormat]::Png) }
            finally { $Bitmap.Dispose() }
        }
    } finally { $Image.Dispose() }
    [xml]$Manifest = Get-Content $Template -Raw
    $Manifest.Package.Identity.SetAttribute('Name', 'Ottrin.TemplateControl')
    $Manifest.Package.Identity.SetAttribute('Publisher', 'CN=Ottrin Template QA, OID.2.25.311729368913984317654407730594956997722=1')
    $Manifest.Package.Identity.SetAttribute('Version', '1.0.0.0')
    $Manifest.Save((Join-Path $Work 'payload\AppxManifest.xml'))
    & $MakeAppx pack /d (Join-Path $Work 'payload') /p (Join-Path $Work 'control.msix') /o | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Native SDK rejected the actual manifest template.' }
    & $MakeAppx unpack /p (Join-Path $Work 'control.msix') /d (Join-Path $Work 'unpacked') /o | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Native SDK unpack failed.' }
    if ((Get-FileHash $Source).Hash -ne (Get-FileHash (Join-Path $Work 'unpacked\bin\ottrin.exe')).Hash) { throw 'SDK round trip altered the control executable.' }
    @{
        status = 'pass'; template_sha256 = (Get-FileHash $Template).Hash
        makeappx_sha256 = (Get-FileHash $MakeAppx).Hash
        scope = 'Actual template accepted by native MakeAppx and unpacked with unchanged control bytes; Windows cmd.exe fixture, not Ottrin runtime or deployment acceptance.'
    } | ConvertTo-Json -Compress
} finally { if (Test-Path $Work) { Remove-Item $Work -Recurse -Force } }
