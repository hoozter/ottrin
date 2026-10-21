$ErrorActionPreference='Stop'
$v=Get-Content C:\OttrinCopyCleanupQA\IO_VOLUME.json -Raw|ConvertFrom-Json
$t=Get-ScheduledTask 'Ottrin Copy Cleanup QA'
if($t.State.ToString()-in@('Running','Queued')){throw 'Owned actor still live'}
if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like'C:\OttrinCopyCleanupQA\*'}).Count){throw 'Owned native process remains'}
$disk=Get-Disk -Number $v.disk_number
$volume=Get-Volume -DriveLetter $v.drive_letter
if($disk.UniqueId-ne$v.disk_unique_id-or$disk.Size-ne512MB-or$disk.IsBoot-or$disk.IsSystem-or$volume.UniqueId-ne$v.volume_unique_id-or$volume.FileSystem-ne'NTFS'){throw 'Owned IO disk identity changed'}
if((Get-Content (Join-Path $v.root '.ottrin-owned-secondary-volume') -Raw).Trim()-ne$v.token){throw 'Owned token mismatch'}
$entries=@(Get-ChildItem -LiteralPath $v.root -Force)
if(@($entries|Where-Object{$_.Name-ne'.ottrin-owned-secondary-volume'}).Count){throw 'Test data remains on secondary disk'}
$baseline=Get-Content C:\OttrinCopyCleanupQA\io-baseline.json -Raw|ConvertFrom-Json
foreach($key in @('preferences','installation')){$item=$baseline.$key;$hash=if(Test-Path -LiteralPath $item.path){(Get-FileHash -LiteralPath $item.path).Hash}else{$null};if($hash-ne$item.hash){throw ('Original '+$key+' changed')}}
# Remove only the executable exported for this run, guarded by its transfer hash.
$hash=(Get-FileHash C:\OttrinCopyCleanupQA\platform.exe).Hash.ToLowerInvariant()
if($hash-ne'EXPECTED_HASH'){throw 'Owned executable changed before teardown'}
Remove-Item C:\OttrinCopyCleanupQA\platform.exe -Force
if(Test-Path C:\OttrinCopyCleanupQA\temp){throw 'Owned native temporary root remains'}
Remove-Item -LiteralPath $v.control -Recurse -Force
Remove-Item C:\OttrinCopyCleanupQA\IO_VOLUME.json,C:\OttrinCopyCleanupQA\io-baseline.json -Force
# This pre-existing manual-only task is retained, with no live actor. Drop the
# task-owned script so it cannot accidentally start a stale native test.
Remove-Item C:\OttrinCopyCleanupQA\actor.ps1 -Force
Set-Disk -Number $v.disk_number -IsOffline $true
@{status='pass';native_processes=0;secondary_fixture_entries=0;baseline_preferences_and_uninstaller_hashes_unchanged=$true;secondary_disk_offline=$true;manual_qa_task_state=$t.State.ToString()}|ConvertTo-Json|Set-Content C:\OttrinCopyCleanupQA\IO_TEARDOWN.json
Get-Content C:\OttrinCopyCleanupQA\IO_TEARDOWN.json -Raw
