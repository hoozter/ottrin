[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string[]]$Files
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$certificateBase64 = $env:OTTRIN_WINDOWS_SIGNING_CERTIFICATE_BASE64
$certificatePassword = $env:OTTRIN_WINDOWS_SIGNING_CERTIFICATE_PASSWORD
$expectedThumbprint = ([string]$env:OTTRIN_WINDOWS_SIGNING_CERTIFICATE_THUMBPRINT -replace '\s', '').ToUpperInvariant()
$timestampUrl = $env:OTTRIN_WINDOWS_SIGNING_TIMESTAMP_URL
if (-not $timestampUrl) {
    $timestampUrl = 'https://timestamp.digicert.com'
}

if (-not $certificateBase64 -or -not $certificatePassword) {
    throw 'The Windows release signing certificate and password are required.'
}
if ($expectedThumbprint -notmatch '^[0-9A-F]{40}$') {
    throw 'OTTRIN_WINDOWS_SIGNING_CERTIFICATE_THUMBPRINT must be the approved 40-hex certificate thumbprint.'
}
if ($Files.Count -eq 0) {
    throw 'At least one Windows release artifact must be supplied.'
}

$signTool = (Get-Command signtool.exe -ErrorAction Stop).Source
$certificatePath = Join-Path ([System.IO.Path]::GetTempPath()) ("ottrin-signing-{0}.pfx" -f [Guid]::NewGuid())
$existingThumbprints = @{}
Get-ChildItem Cert:\CurrentUser\My | ForEach-Object { $existingThumbprints[$_.Thumbprint] = $true }
$importedCertificates = @()

try {
    try {
        $certificateBytes = [Convert]::FromBase64String($certificateBase64)
    } catch {
        throw 'The Windows release signing certificate is not valid base64.'
    }
    [System.IO.File]::WriteAllBytes($certificatePath, $certificateBytes)

    $securePassword = ConvertTo-SecureString -String $certificatePassword -AsPlainText -Force
    $importedCertificates = @(Import-PfxCertificate `
        -FilePath $certificatePath `
        -CertStoreLocation Cert:\CurrentUser\My `
        -Password $securePassword `
        -Exportable:$false)

    $signingCertificate = @($importedCertificates | Where-Object { $_.HasPrivateKey })
    if ($signingCertificate.Count -ne 1) {
        throw 'The signing bundle must import exactly one certificate with a private key.'
    }
    $actualThumbprint = ($signingCertificate[0].Thumbprint -replace '\s', '').ToUpperInvariant()
    if ($actualThumbprint -ne $expectedThumbprint) {
        throw 'The Windows signing certificate does not match the approved certificate thumbprint.'
    }

    foreach ($file in $Files) {
        $resolvedFile = (Resolve-Path -LiteralPath $file -ErrorAction Stop).Path
        & $signTool sign /sha1 $actualThumbprint /fd SHA256 /tr $timestampUrl /td SHA256 $resolvedFile
        if ($LASTEXITCODE -ne 0) {
            throw "Authenticode signing failed for $file."
        }
        & $signTool verify /pa /all $resolvedFile
        if ($LASTEXITCODE -ne 0) {
            throw "Authenticode verification failed for $file."
        }
    }
} finally {
    Remove-Item -LiteralPath $certificatePath -Force -ErrorAction SilentlyContinue
    foreach ($certificate in $importedCertificates) {
        if (-not $existingThumbprints.ContainsKey($certificate.Thumbprint)) {
            Remove-Item -LiteralPath $certificate.PSPath -Force -ErrorAction SilentlyContinue
        }
    }
}
