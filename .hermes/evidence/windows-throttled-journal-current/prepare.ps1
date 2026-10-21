$ErrorActionPreference='Stop'
$t=Get-ScheduledTask 'Ottrin Copy Cleanup QA'
if($t.State.ToString()-in@('Running','Queued')-or[int]$t.Principal.RunLevel-ne0){throw 'Owned limited actor is unavailable'}
if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like'C:\OttrinCopyCleanupQA\*'}).Count){throw 'Owned native process remains'}
$control='C:\OttrinCopyCleanupQA\io-control'
if(Test-Path $control){throw 'Control directory already exists'}
$baseline=@{}
foreach($pair in @(@('preferences','C:\Users\campbell\AppData\Roaming\ottrin\config.json'),@('installation','C:\Users\campbell\AppData\Local\Programs\Ottrin\Uninstall.exe'))){$baseline[$pair[0]]=@{path=$pair[1];hash=$(if(Test-Path -LiteralPath $pair[1]){(Get-FileHash -LiteralPath $pair[1]).Hash}else{$null})}}
$baseline|ConvertTo-Json -Depth 4|Set-Content C:\OttrinCopyCleanupQA\io-baseline.json
$disks=@(Get-Disk|Where-Object{$_.SerialNumber-eq'ottrin-throttled-jou'})
if($disks.Count-ne1-or$disks[0].Size-ne512MB-or$disks[0].IsBoot-or$disks[0].IsSystem-or$disks[0].PartitionStyle-ne'RAW'){throw 'Owned blank secondary disk guard failed'}
$number=$disks[0].Number;$identity=$disks[0].UniqueId
function OwnedDisk {$d=Get-Disk -Number $number;if($d.UniqueId-ne$identity-or$d.IsBoot-or$d.IsSystem-or$d.Size-ne512MB){throw 'Owned secondary disk association changed'};$d}
$null=OwnedDisk
Initialize-Disk -Number $number -PartitionStyle GPT -Confirm:$false|Out-Null
$null=OwnedDisk
$partition=New-Partition -DiskNumber $number -UseMaximumSize -AssignDriveLetter
$null=OwnedDisk
Format-Volume -Partition $partition -FileSystem NTFS -AllocationUnitSize 4096 -NewFileSystemLabel 'OttrinIOQA' -Force -Confirm:$false|Out-Null
$volume=Get-Volume -DriveLetter $partition.DriveLetter
if($volume.FileSystem-ne'NTFS'-or$volume.FileSystemLabel-ne'OttrinIOQA'){throw 'Owned NTFS format failed'}
$root="$($partition.DriveLetter):\OttrinIOQA"
New-Item -ItemType Directory $root|Out-Null
$sid=(Get-LocalUser OttrinCopyQA).SID
$acl=[Security.AccessControl.DirectorySecurity]::new()
$acl.SetSecurityDescriptorSddlForm("O:$($sid.Value)D:P(A;OICI;FA;;;$($sid.Value))(A;OICI;FA;;;SY)")
Set-Acl -LiteralPath $root -AclObject $acl
'ccd1342584ce85acde55ba8756c1e353'|Set-Content -LiteralPath (Join-Path $root '.ottrin-owned-secondary-volume')
New-Item -ItemType Directory $control|Out-Null
@{root=$root;control=$control;token='ccd1342584ce85acde55ba8756c1e353';actor_sid=$sid.Value;disk_number=$number;disk_unique_id=$identity;volume_unique_id=$volume.UniqueId;drive_letter=$partition.DriveLetter.ToString()}|ConvertTo-Json|Set-Content C:\OttrinCopyCleanupQA\IO_VOLUME.json
Get-Content C:\OttrinCopyCleanupQA\IO_VOLUME.json -Raw
