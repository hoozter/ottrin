$ErrorActionPreference='Stop';$root='ROOT';$result=@{dispatch_id='TOKEN';status='cleaning'}
try {
 if($env:USERNAME-ne'OttrinCopyQA'-or(Get-Content "$root\guard" -Raw)-ne'TOKEN'){throw 'Wrong cleanup ownership'}
 if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like'C:\OttrinCopyCleanupQA\*'}).Count){throw 'Owned executable remains'}
 $claims="$root\destination\.ottrin-copy-target-claims";$prep=Join-Path $claims ('.preparing-'+('d'*32));$key="$claims\short-names.ottrin-claims\copy.ottrin-claim"
 if('PHASE'-eq'Prepared'){[IO.File]::WriteAllBytes('C:\OttrinCopyCleanupQA\results\POWER_PREPARATION.bin',[IO.File]::ReadAllBytes($prep))}
 [IO.File]::WriteAllBytes('C:\OttrinCopyCleanupQA\results\POWER_CANONICAL.bin',[IO.File]::ReadAllBytes($key))
 Remove-Item -LiteralPath $root -Recurse -Force
 $result.root_absent=-not(Test-Path $root);$result.status='pass'
} catch {$result.status='fail';$result.error=$_.Exception.Message}
$result|ConvertTo-Json|Set-Content C:\OttrinCopyCleanupQA\results\POWER_CLEANUP.json
if($result.status-ne'pass'){exit 1}
