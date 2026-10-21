$ErrorActionPreference='Stop'
$root='C:\OttrinWindowQA';$name='OttrinWindowQA'
if(Test-Path $root){throw 'Owned root already exists'}
if(Get-LocalUser $name -ErrorAction SilentlyContinue){throw 'Owned account already exists'}
if((Get-CimInstance Win32_ComputerSystem).UserName){throw 'Console is occupied'}
$key=[Microsoft.Win32.Registry]::LocalMachine.OpenSubKey('SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon',$false)
if('DefaultPassword'-in$key.GetValueNames()){throw 'Existing autologon credential must be preserved'}
$backup=@{}
foreach($n in @('DefaultUserName','DefaultDomainName','AutoAdminLogon')){if($n-in$key.GetValueNames()){$backup[$n]=@{present=$true;value=$key.GetValue($n,$null,[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames);kind=$key.GetValueKind($n).ToString()}}else{$backup[$n]=@{present=$false}}};$key.Dispose()
New-Item -ItemType Directory $root|Out-Null
$acl=[Security.AccessControl.DirectorySecurity]::new();$acl.SetAccessRuleProtection($true,$false)
foreach($s in @('S-1-5-18','S-1-5-32-544')){$acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new($s),'FullControl','ContainerInherit,ObjectInherit','None','Allow'))};Set-Acl $root $acl
New-Item -ItemType Directory "$root\system","$root\results","$root\cases","$root\actor-temp"|Out-Null
$backup|ConvertTo-Json -Depth 6|Set-Content "$root\system\winlogon-backup.json"
$baseline=@{};foreach($p in @('C:\Users\campbell\AppData\Roaming\ottrin\config.json','C:\Users\campbell\AppData\Local\Programs\Ottrin\Uninstall.exe')){if(Test-Path $p){$baseline[$p]=(Get-FileHash $p).Hash}else{$baseline[$p]=$null}};$baseline|ConvertTo-Json|Set-Content "$root\system\baseline.json"
# The temporary password is generated inside the guest and is never returned.
$bytes=New-Object byte[] 24;$rng=[Security.Cryptography.RandomNumberGenerator]::Create();$rng.GetBytes($bytes);$rng.Dispose();$plain=[Convert]::ToBase64String($bytes)+'Aa1!'
$u=New-LocalUser $name -Password (ConvertTo-SecureString $plain -AsPlainText -Force) -Description 'Ottrin owned packaged workflow QA' -PasswordNeverExpires
Add-LocalGroupMember -SID 'S-1-5-32-545' -Member $u
@{account=$name;sid=$u.SID.Value}|ConvertTo-Json|Set-Content "$root\actor-config.json"
$acl=Get-Acl "$root\system";$acl.SetAccessRuleProtection($true,$true);Set-Acl "$root\system" $acl
$acl=Get-Acl $root;$acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($u.SID,'ReadAndExecute','ContainerInherit,ObjectInherit','None','Allow'));Set-Acl $root $acl
foreach($d in @('results','cases','actor-temp')){$acl=Get-Acl "$root\$d";$acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($u.SID,'Modify','ContainerInherit,ObjectInherit','None','Allow'));Set-Acl "$root\$d" $acl}
$a=New-ScheduledTaskAction -Execute 'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe' -Argument '-NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File C:\OttrinWindowQA\actor.ps1'
$principal=New-ScheduledTaskPrincipal -UserId $u.SID.Value -LogonType Interactive -RunLevel Limited
Register-ScheduledTask -TaskName 'Ottrin Window Input QA' -Action $a -Principal $principal|Out-Null
$key=[Microsoft.Win32.Registry]::LocalMachine.OpenSubKey('SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon',$true)
if('DefaultPassword'-in$key.GetValueNames()){throw 'Autologon changed during preparation'}
$key.SetValue('DefaultUserName',$name);$key.SetValue('DefaultDomainName',$env:COMPUTERNAME);$key.SetValue('DefaultPassword',$plain);$key.SetValue('AutoAdminLogon','1');$key.Dispose();$plain=$null
Write-Output 'Owned ordinary account ready; protected restoration data saved.'
