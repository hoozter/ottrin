$ErrorActionPreference='Stop'
$r='C:\OttrinCopyCleanupQA'
$v=Get-Content "$r\SECONDARY_VOLUME.json" -Raw|ConvertFrom-Json
if($v.image-ne"$r\secondary-volume\owned.vhdx"-or$v.disk_size-ne256MB){throw 'Unexpected fixture identity'}
$disk=Get-DiskImage -ImagePath $v.image|Get-Disk
if(@($disk).Count-ne1-or$disk.Number-ne$v.disk_number-or$disk.UniqueId-ne$v.disk_unique_id-or$disk.IsBoot-or$disk.IsSystem-or$disk.Size-ne256MB-or$v.original_disk_numbers-contains$disk.Number){throw 'Owned disk association guard failed'}
$volume=Get-Volume -DriveLetter $v.drive_letter
if($volume.UniqueId-ne$v.volume_unique_id-or$volume.FileSystem-ne'NTFS'-or$volume.FileSystemLabel-ne'OttrinCopyQA'){throw 'Owned volume identity changed'}
if((Get-Content -LiteralPath (Join-Path $v.root '.ottrin-owned-secondary-volume') -Raw).Trim()-ne$v.token){throw 'Owned marker changed'}
$entries=@(Get-ChildItem -LiteralPath $v.root -Force)
if($entries.Count-ne1-or$entries[0].Name-ne'.ottrin-owned-secondary-volume'){throw 'Test left secondary-volume data'}
if((Get-ScheduledTask 'Ottrin Copy Cleanup QA').State.ToString()-in@('Running','Queued')){throw 'Actor still live'}
if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like"$r\*"}).Count){throw 'Owned native process still live'}
$before=@(Get-Content "$r\secondary-volume\before-disks.json" -Raw|ConvertFrom-Json)
Dismount-DiskImage -ImagePath $v.image
if((Get-DiskImage -ImagePath $v.image).Attached){throw 'Owned disk remains attached'}
$after=@(Get-Disk|Select-Object Number,UniqueId,Size,IsBoot,IsSystem)
if($after.Count-ne$before.Count){throw 'Original disk count changed'}
foreach($b in $before){$a=@($after|Where-Object{$_.Number-eq$b.Number});if($a.Count-ne1-or$a[0].UniqueId-ne$b.UniqueId-or$a[0].Size-ne$b.Size-or$a[0].IsBoot-ne$b.IsBoot-or$a[0].IsSystem-ne$b.IsSystem){throw 'Original disk identity changed'}}
Remove-Item -LiteralPath "$r\secondary-volume" -Recurse -Force
Remove-Item -LiteralPath "$r\SECONDARY_VOLUME.json" -Force
if(Test-Path "$r\prepare-secondary.ps1"){Remove-Item -LiteralPath "$r\prepare-secondary.ps1" -Force}
@{status='pass';secondary_test_entries_absent=$true;owned_disk_detached=$true;owned_image_removed=$true;original_disks_unchanged=$true;original_disk_count=$after.Count}|ConvertTo-Json|Set-Content "$r\SECONDARY_TEARDOWN.json"
Get-Content "$r\SECONDARY_TEARDOWN.json"
