$ErrorActionPreference='Stop'
$r='C:\OttrinCopyCleanupQA\secondary-volume'
if(Test-Path -LiteralPath $r){throw 'Owned secondary fixture namespace already exists'}
$before=@(Get-Disk|Select-Object Number,UniqueId,Size,IsBoot,IsSystem)
if((Get-PSDrive C).Free-lt1GB){throw 'Insufficient guest staging space'}
New-Item -ItemType Directory -Path $r|Out-Null
$before|ConvertTo-Json -Depth 4|Set-Content "$r\before-disks.json"
$image=Join-Path $r 'owned.vhdx'
'create vdisk file="'+$image+'" maximum=256 type=expandable'|Set-Content "$r\create.txt"
& diskpart.exe /s "$r\create.txt"|Set-Content "$r\create.log"
if($LASTEXITCODE-ne0-or-not(Test-Path -LiteralPath $image)){throw 'VHD creation failed'}
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
Format-Volume -Partition $partition -FileSystem NTFS -AllocationUnitSize 4096 -NewFileSystemLabel 'OttrinCopyQA' -Force -Confirm:$false|Out-Null
$volume=Get-Volume -DriveLetter $partition.DriveLetter
if($volume.FileSystem-ne'NTFS'-or$volume.FileSystemLabel-ne'OttrinCopyQA'){throw 'Owned volume format not confirmed'}
$root="$($partition.DriveLetter):\OttrinCopyQA"
New-Item -ItemType Directory -Path $root|Out-Null
$sid=(Get-LocalUser -Name OttrinCopyQA).SID
$acl=[Security.AccessControl.DirectorySecurity]::new()
$acl.SetSecurityDescriptorSddlForm("O:$($sid.Value)D:P(A;OICI;FA;;;$($sid.Value))(A;OICI;FA;;;SY)")
Set-Acl -LiteralPath $root -AclObject $acl
$token=[Guid]::NewGuid().ToString('N')
$token|Set-Content -LiteralPath (Join-Path $root '.ottrin-owned-secondary-volume')
$fixture=@{status='ready';image=$image;root=$root;disk_number=$number;disk_unique_id=$identity;disk_size=$disk.Size;volume_unique_id=$volume.UniqueId;drive_letter=$partition.DriveLetter.ToString();filesystem=$volume.FileSystem;label=$volume.FileSystemLabel;token=$token;actor_sid=$sid.Value;original_disk_numbers=@($before.Number)}
$fixture|ConvertTo-Json -Depth 5|Set-Content 'C:\OttrinCopyCleanupQA\SECONDARY_VOLUME.json'
$fixture|ConvertTo-Json -Depth 5
