param([Parameter(Mandatory=$true)][ValidateSet('prepare','remove')][string]$Action)
$ErrorActionPreference='Stop'
$r='C:\OttrinCopyCleanupQA\cross-move-volume'
if($Action-eq'remove'){
 if((Get-ScheduledTask 'Ottrin Copy Cleanup QA').State.ToString()-in@('Running','Queued')){throw 'Owned actor still live'}
 if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like'C:\OttrinCopyCleanupQA\*'}).Count){throw 'Owned native process remains'}
 $v=Get-Content "$r\volume.json" -Raw|ConvertFrom-Json
 if($v.image-ne"$r\owned.vhdx"-or$v.disk_size-ne256MB){throw 'Unexpected fixture ownership'}
 $image=Get-DiskImage -ImagePath $v.image;$disk=$image|Get-Disk;$volume=Get-Volume -DriveLetter $v.drive_letter
 if(@($disk).Count-ne1-or$disk.Number-ne$v.disk_number-or$disk.UniqueId-ne$v.disk_unique_id-or$disk.IsBoot-or$disk.IsSystem-or$volume.UniqueId-ne$v.volume_unique_id-or$volume.FileSystemLabel-ne'OttrinMoveQA'-or((Get-Content (Join-Path $v.root '.ottrin-owned-secondary-volume') -Raw).Trim()-ne$v.token)){throw 'Owned disk/volume identity changed'}
 Dismount-DiskImage -ImagePath $v.image
 if((Get-DiskImage -ImagePath $v.image).Attached){throw 'Owned VHD still attached'}
 Remove-Item -LiteralPath $v.image -Force
 $before=Get-Content "$r\before-disks.json" -Raw|ConvertFrom-Json;$after=@(Get-Disk|Select-Object Number,UniqueId,Size,IsBoot,IsSystem)
 if(@($before).Count-ne$after.Count){throw 'Disk inventory not restored'}
 foreach($entry in $before){$actual=@($after|Where-Object Number -eq $entry.Number);if($actual.Count-ne1-or$actual[0].UniqueId-ne$entry.UniqueId-or$actual[0].Size-ne$entry.Size){throw 'Original disk changed'}}
 Remove-Item -LiteralPath $r -Recurse -Force
 @{status='pass';owned_vhd_detached_and_removed=$true;original_disk_inventory_restored=$true;retained_vm_disks_preserved=$true}|ConvertTo-Json|Set-Content 'C:\OttrinCopyCleanupQA\CrossMoveVolumeTeardown.json'
 return
}
if(Test-Path -LiteralPath $r){throw 'Owned secondary fixture namespace already exists'}
$before=@(Get-Disk|Select-Object Number,UniqueId,Size,IsBoot,IsSystem)
if((Get-PSDrive C).Free-lt1GB){throw 'Insufficient guest staging space'}
New-Item -ItemType Directory -Path $r|Out-Null
$before|ConvertTo-Json -Depth 4|Set-Content "$r\before-disks.json"
$image=Join-Path $r 'owned.vhdx'
'create vdisk file="'+$image+'" maximum=256 type=expandable'|Set-Content "$r\create.txt"
& diskpart.exe /s "$r\create.txt"|Set-Content "$r\create.log"
if($LASTEXITCODE-ne0-or-not(Test-Path -LiteralPath $image)){throw 'VHD creation failed'}
$sid=(Get-LocalUser -Name OttrinCopyQA).SID
$acl=Get-Acl -LiteralPath $image
$acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($sid,'FullControl','Allow'))
Set-Acl -LiteralPath $image -AclObject $acl
Mount-DiskImage -ImagePath $image -Access ReadWrite|Out-Null
$disk=Get-DiskImage -ImagePath $image|Get-Disk
if(@($disk).Count-ne1-or$disk.IsBoot-or$disk.IsSystem-or$disk.Size-ne256MB-or$disk.PartitionStyle-ne'RAW'-or$before.Number-contains$disk.Number){throw 'New disk identity/blank-state guard failed'}
$number=$disk.Number;$identity=$disk.UniqueId
@{image=$image;number=$number;unique_id=$identity;before=$before}|ConvertTo-Json -Depth 5|Set-Content "$r\attached.json"
function OwnedDisk {
 $current=Get-DiskImage -ImagePath $image|Get-Disk
 if(@($current).Count-ne1-or$current.Number-ne$number-or$current.UniqueId-ne$identity-or$current.IsBoot-or$current.IsSystem-or$current.Size-ne256MB){throw 'Owned disk association changed'}
 $current
}
$null=OwnedDisk
Initialize-Disk -Number $number -PartitionStyle GPT -Confirm:$false|Out-Null
$null=OwnedDisk
$partition=New-Partition -DiskNumber $number -UseMaximumSize -AssignDriveLetter
$null=OwnedDisk
Format-Volume -Partition $partition -FileSystem NTFS -AllocationUnitSize 65536 -NewFileSystemLabel 'OttrinMoveQA' -Force -Confirm:$false|Out-Null
$volume=Get-Volume -DriveLetter $partition.DriveLetter
if($volume.FileSystem-ne'NTFS'-or$volume.FileSystemLabel-ne'OttrinMoveQA'-or$volume.AllocationUnitSize-ne65536){throw 'Owned volume format not confirmed'}
$root="$($partition.DriveLetter):\OttrinCopyQA"
New-Item -ItemType Directory -Path $root|Out-Null
$sid=(Get-LocalUser -Name OttrinCopyQA).SID
$acl=[Security.AccessControl.DirectorySecurity]::new()
$acl.SetSecurityDescriptorSddlForm("O:$($sid.Value)D:P(A;OICI;FA;;;$($sid.Value))(A;OICI;FA;;;SY)")
Set-Acl -LiteralPath $root -AclObject $acl
$token=[Guid]::NewGuid().ToString('N')
$token|Set-Content -LiteralPath (Join-Path $root '.ottrin-owned-secondary-volume')
$primary=Get-Volume -DriveLetter C;if($primary.FileSystem-ne'NTFS'-or$primary.AllocationUnitSize-eq$volume.AllocationUnitSize){throw 'Primary NTFS allocation must differ'}
$fixture=@{status='ready';primary_allocation_unit=$primary.AllocationUnitSize;secondary_allocation_unit=$volume.AllocationUnitSize;image=$image;root=$root;disk_number=$number;disk_unique_id=$identity;disk_size=$disk.Size;volume_unique_id=$volume.UniqueId;drive_letter=$partition.DriveLetter.ToString();filesystem=$volume.FileSystem;label=$volume.FileSystemLabel;token=$token;actor_sid=$sid.Value;original_disk_numbers=@($before.Number)}
$fixture|ConvertTo-Json -Depth 5|Set-Content "$r\volume.json"
$fixture|ConvertTo-Json -Depth 5
