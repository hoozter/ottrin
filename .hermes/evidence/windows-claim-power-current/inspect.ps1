$ErrorActionPreference='Stop';$result=@{status='inspecting';dispatch_id='TOKEN';root='ROOT';binary_sha256='BINARY'}
try {
 $id=[Security.Principal.WindowsIdentity]::GetCurrent()
 if($env:USERNAME-ne'OttrinCopyQA'-or([Security.Principal.WindowsPrincipal]::new($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Wrong or elevated inspector'}
 $root='ROOT';if((Get-Content "$root\guard" -Raw)-ne'TOKEN'){throw 'Root ownership mismatch'}
 $result.ordinary_user=$true;$env:TEMP="$root\compiler";$env:TMP=$env:TEMP
 . C:\OttrinCopyCleanupQA\power-sync.ps1
 $claims="$root\destination\.ottrin-copy-target-claims";$prep=Join-Path $claims ('.preparing-'+('d'*32));$foreign=Join-Path $claims ('.preparing-'+('f'*32))
 $result.preparation_exists=Test-Path $prep
 if($result.preparation_exists){$result.preparation_sha256=Read-SharedHash $prep;$result.preparation_length=(Get-Item $prep).Length}
 $result.source_sha256=Read-SharedHash "$root\source\nested\file";$result.public_sha256=Read-SharedHash "$root\destination\user-data";$result.foreign_sha256=Read-SharedHash $foreign
 $result.canonical_exists=Test-Path "$claims\short-names.ottrin-claims\copy.ottrin-claim"
 $result.claim_root_names=@(Get-ChildItem $claims -Force|Select-Object -ExpandProperty Name)
 $result.status='observed-after-cut'
} catch {$result.status='fail';$result.error=$_.Exception.Message}
$result|ConvertTo-Json -Depth 4|Set-Content C:\OttrinCopyCleanupQA\results\POWER_AFTER.json.tmp
Move-Item C:\OttrinCopyCleanupQA\results\POWER_AFTER.json.tmp C:\OttrinCopyCleanupQA\results\POWER_AFTER.json -Force
if($result.status-ne'observed-after-cut'){exit 1}
