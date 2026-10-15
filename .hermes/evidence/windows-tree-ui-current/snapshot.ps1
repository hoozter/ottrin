function FileId($path) {
    $text = (& fsutil file queryfileid $path | Out-String)
    $id = [regex]::Match($text, '0x[0-9a-fA-F]+').Value
    Assert ($id.Length -gt 10) "Missing native identity: $path"
    return $id
}
function Snapshot($path) {
    $root = Get-Item -LiteralPath $path -Force
    $nodes = @($root)
    if ($root.PSIsContainer) { $nodes += @(Get-ChildItem -LiteralPath $path -Recurse -Force) }
    foreach ($node in $nodes) {
        Assert (-not ($node.Attributes -band [IO.FileAttributes]::ReparsePoint)) 'This fixture admits plain trees only'
        $relative = if ($node.FullName -ceq $root.FullName) { '.' } else { $node.FullName.Substring($root.FullName.Length + 1) }
        $hash = if ($node.PSIsContainer) { $null } else { (Get-FileHash -LiteralPath $node.FullName -Algorithm SHA256).Hash }
        [pscustomobject]@{path=$relative; kind=$(if ($node.PSIsContainer) {'directory'} else {'file'}); sha256=$hash; id=(FileId $node.FullName)}
    }
}
function EqualSnapshot($actual, $expected, [bool]$identities) {
    $left = @($actual | ForEach-Object { $_.path + '|' + $_.kind + '|' + $_.sha256 + $(if ($identities) {'|' + $_.id} else {''}) } | Sort-Object)
    $right = @($expected | ForEach-Object { $_.path + '|' + $_.kind + '|' + $_.sha256 + $(if ($identities) {'|' + $_.id} else {''}) } | Sort-Object)
    return ($left.Count -eq $right.Count -and ($left -join "`n") -ceq ($right -join "`n"))
}
function NativePath($record) {
    Assert ($record.encoding -eq 'windows-utf16-le') 'Unexpected native path encoding'
    Assert (($record.units_hex.Length % 4) -eq 0) 'Malformed native path'
    $units = for ($i=0; $i -lt $record.units_hex.Length; $i+=4) { [char][Convert]::ToUInt16($record.units_hex.Substring($i,4),16) }
    $path = -join $units
    if ($path.StartsWith('\\?\')) { $path = $path.Substring(4) }
    return $path
}
function Retained($entry) {
    $matches = @()
    foreach ($file in Get-ChildItem "$env:LOCALAPPDATA\Ottrin\State\moves" -File | Where-Object Extension -in @('.journal','.cross-journal')) {
        $record = Get-Content -LiteralPath $file.FullName -Raw -Encoding UTF8 | ConvertFrom-Json
        if ((NativePath $record.source) -ceq $entry.source -and $record.retained_displaced) {
            Assert ($record.phase -eq 'published') 'Retained transaction not published'
            $path = NativePath $record.retained_displaced
            Assert (EqualSnapshot @(Snapshot $path) @($entry.destination_before) $true) 'Displaced tree bytes/identities differ'
            $stage = NativePath $record.transaction_directory
            $local = @(Get-ChildItem -LiteralPath $stage -File -Force | Where-Object Name -like '*journal*')
            Assert ($local.Count -eq 1) 'Expected one paired local journal'
            Assert ([IO.File]::ReadAllText($local[0].FullName).Trim() -ceq [IO.File]::ReadAllText($file.FullName).Trim()) 'Paired journal bytes differ'
            $matches += @{path=$path; stage=$stage; central=$file.FullName; local=$local[0].FullName; journal_sha256=(Get-FileHash -LiteralPath $file.FullName).Hash; snapshot=@(Snapshot $path)}
        }
    }
    Assert ($matches.Count -eq 1) 'Expected exactly one retained transaction for this source'
    return $matches[0]
}
function VerifyCase($case) {
    $sourceNames = @()
    $destinationNames = @()
    foreach ($entry in $case.entries) {
        if ($entry.moved) {
            Assert (-not (Test-Path -LiteralPath $entry.source)) 'Moved source still exists'
            if (-not $entry.published) {
                $candidates = @(Get-ChildItem -LiteralPath $case.destination -Force | Where-Object { $_.Name -notin @($case.entries | ForEach-Object name) -and $_.Name -notlike '.ottrin-move-*' })
                $found = @($candidates | Where-Object { EqualSnapshot @(Snapshot $_.FullName) @($entry.source_before) $false })
                Assert ($found.Count -eq 1) 'Expected exactly one Keep Both publication'
                $entry.published = $found[0].FullName
            }
            Assert (EqualSnapshot @(Snapshot $entry.published) @($entry.source_before) ($case.volume -eq 'same')) 'Moved tree bytes/shape/identity differ'
            $destinationNames += Split-Path $entry.published -Leaf
        } else {
            Assert (EqualSnapshot @(Snapshot $entry.source) @($entry.source_before) $true) 'Skipped/cancelled source changed'
            $sourceNames += $entry.name
        }
        if (@($entry.destination_before).Count -gt 0) {
            if (-not $entry.moved -or $case.choice -eq 'rename') {
                Assert (EqualSnapshot @(Snapshot $entry.original_target) @($entry.destination_before) $true) 'Existing destination changed'
                $destinationNames += $entry.name
            } else {
                $entry.retained = Retained $entry
                $destinationNames += Split-Path $entry.retained.stage -Leaf
            }
        } elseif (-not $entry.moved) {
            Assert (-not (Test-Path -LiteralPath $entry.original_target)) 'Cancelled unoccupied entry was published'
        }
    }
    $actualSource = @(Get-ChildItem -LiteralPath $case.source_parent -Force | ForEach-Object Name | Sort-Object)
    $expectedSource = @($sourceNames | Sort-Object)
    Assert (($actualSource -join "`n") -ceq ($expectedSource -join "`n")) 'Source parent contains claimed data or unexpected entries'
    $actualDest = @(Get-ChildItem -LiteralPath $case.destination -Force | ForEach-Object Name | Sort-Object)
    $expectedDest = @($destinationNames | Sort-Object -Unique)
    Assert (($actualDest -join "`n") -ceq ($expectedDest -join "`n")) 'Destination contains an unfinished stage or unexpected entries'
    return @{name=$case.name; status='pass'; source_entries=$actualSource.Count; destination_entries=$actualDest.Count; moved=@($case.entries | Where-Object moved).Count; retained=@($case.entries | Where-Object retained).Count}
}
