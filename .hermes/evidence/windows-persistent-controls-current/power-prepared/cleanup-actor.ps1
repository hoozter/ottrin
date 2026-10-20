$ErrorActionPreference='Stop';$root='C:\OttrinCopyCleanupQA\power-011eba69aeddd4b9bec2c62b234d31e2';$result=@{dispatch_id='011eba69aeddd4b9bec2c62b234d31e2';status='cleaning'}
try {
 if($env:USERNAME-ne'OttrinCopyQA'-or(Get-Content "$root\guard" -Raw)-ne'011eba69aeddd4b9bec2c62b234d31e2'){throw 'Wrong cleanup ownership'}
 if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like'C:\OttrinCopyCleanupQA\*'}).Count){throw 'Owned executable remains'}
 $claims="$root\destination\.ottrin-copy-target-claims";$prep=Join-Path $claims ('.preparing-'+('d'*32));$key="$claims\short-names.ottrin-claims\copy.ottrin-claim"
 [IO.File]::WriteAllBytes('C:\OttrinCopyCleanupQA\results\POWER_CANONICAL.bin',[IO.File]::ReadAllBytes($key))
 Remove-Item -LiteralPath $root -Recurse -Force
 $result.root_absent=-not(Test-Path $root);$result.status='pass'
} catch {$result.status='fail';$result.error=$_.Exception.Message}
$result|ConvertTo-Json|Set-Content C:\OttrinCopyCleanupQA\results\POWER_CLEANUP.json
if($result.status-ne'pass'){exit 1}
