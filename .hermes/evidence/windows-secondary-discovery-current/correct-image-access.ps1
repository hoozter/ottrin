$r='C:\OttrinCopyCleanupQA';$v=Get-Content "$r\SECONDARY_VOLUME.json" -Raw|ConvertFrom-Json
if($v.image-ne"$r\secondary-volume\owned.vhdx"){throw 'Wrong owned image'}
$disk=Get-DiskImage -ImagePath $v.image|Get-Disk
if($disk.Number-ne$v.disk_number-or$disk.UniqueId-ne$v.disk_unique_id-or$disk.IsBoot-or$disk.IsSystem-or$disk.Size-ne256MB-or$v.original_disk_numbers-contains$disk.Number){throw 'Owned disk guard failed'}
if((Get-ScheduledTask 'Ottrin Copy Cleanup QA').State-eq'Running'){throw 'Actor live'}
if(@(Get-ChildItem -LiteralPath $v.root -Force).Count-ne1){throw 'Fixture data remains'}
Dismount-DiskImage -ImagePath $v.image|Out-Null
if((Get-DiskImage -ImagePath $v.image).Attached){throw 'Detach failed'}
$acl=Get-Acl -LiteralPath $v.image
$acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new($v.actor_sid),'FullControl','Allow'))
Set-Acl -LiteralPath $v.image -AclObject $acl
Mount-DiskImage -ImagePath $v.image -Access ReadWrite|Out-Null
$disk=Get-DiskImage -ImagePath $v.image|Get-Disk
if($disk.UniqueId-ne$v.disk_unique_id-or$disk.IsBoot-or$disk.IsSystem-or$disk.Size-ne256MB){throw 'Reattachment identity changed'}
$volume=Get-Volume -DriveLetter $v.drive_letter
if($volume.UniqueId-ne$v.volume_unique_id-or(Get-Content -LiteralPath (Join-Path $v.root '.ottrin-owned-secondary-volume') -Raw).Trim()-ne$v.token){throw 'Volume identity changed'}
$v.disk_number=$disk.Number;$v|ConvertTo-Json -Depth 5|Set-Content "$r\SECONDARY_VOLUME.json"
@{status='pass';owned_image=$v.image;image_acl=(Get-Acl -LiteralPath $v.image).Sddl;same_volume=$true;actor_permission='FullControl';production_code_changed=$false}|ConvertTo-Json|Set-Content "$r\IMAGE_ACCESS_CORRECTION.json"
