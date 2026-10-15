$OttrinSearchRuntimeManifest = Join-Path $PSScriptRoot 'search-runtime.json'

function Resolve-SearchRuntime {
    param([string]$BinaryDestination, [string]$NoticeDestination)
    $manifest = Get-Content $OttrinSearchRuntimeManifest -Raw | ConvertFrom-Json
    $runtime = $manifest.platforms.'windows-x86_64'
    $work = Join-Path ([IO.Path]::GetTempPath()) ('ottrin-search-runtime-' + [guid]::NewGuid())
    New-Item -ItemType Directory $work | Out-Null
    try {
        $archive = Join-Path $work 'runtime.zip'
        Get-VerifiedFile $runtime.url $runtime.sha256 $archive
        Expand-Archive -LiteralPath $archive -DestinationPath (Join-Path $work 'extract')
        $source = Join-Path (Join-Path $work 'extract') $runtime.prefix
        $binary = Join-Path $source $runtime.binary
        if ((Get-FileHash $binary).Hash.ToLowerInvariant() -ne $runtime.binary_sha256) {
            throw 'Search-runtime binary checksum mismatch'
        }
        New-Item -ItemType Directory -Force $BinaryDestination, $NoticeDestination | Out-Null
        Copy-Item $binary (Join-Path $BinaryDestination $runtime.binary)
        foreach ($license in $manifest.licenses) {
            Copy-Item (Join-Path $source $license) (Join-Path $NoticeDestination $license)
        }
        foreach ($entry in $manifest.source_notices.PSObject.Properties) {
            $file = Join-Path (Split-Path $OttrinSearchRuntimeManifest) $entry.Value.file
            if ((Get-FileHash $file).Hash.ToLowerInvariant() -ne $entry.Value.sha256) {
                throw "Search-runtime notice checksum mismatch: $($entry.Name)"
            }
            Copy-Item $file (Join-Path $NoticeDestination $entry.Name)
        }
        $notice = "ripgrep $($manifest.version)`nSource archive: $($runtime.url)`nArchive SHA-256: $($runtime.sha256)`nBinary SHA-256: $($runtime.binary_sha256)`n"
        [IO.File]::WriteAllText((Join-Path $NoticeDestination 'MANIFEST.txt'), $notice, [Text.UTF8Encoding]::new($false))
    } finally {
        Remove-Item -LiteralPath $work -Recurse -Force
    }
}
