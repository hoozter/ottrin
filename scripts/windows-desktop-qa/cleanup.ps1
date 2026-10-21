$ErrorActionPreference='Stop';$root='C:\OttrinWindowQA'
$owned=Get-Content "$root\actor-config.json" -Raw|ConvertFrom-Json
$u=Get-LocalUser $owned.account
if($u.SID.Value-ne$owned.sid-or$u.Description-ne'Ottrin owned packaged workflow QA'){throw 'Wrong account ownership'}
$task=Get-ScheduledTask 'Ottrin Window Input QA'
if($task.State.ToString()-in@('Running','Queued')){throw 'Owned actor still active'}
# Emergency cleanup may terminate only executables under this owned payload.
foreach($p in @(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like"$root\payload\*"})){Stop-Process -Id $p.ProcessId -Force -ErrorAction Stop}
$b=Get-Content "$root\system\baseline.json" -Raw|ConvertFrom-Json
foreach($e in $b.PSObject.Properties){if($e.Value){if((Get-FileHash $e.Name).Hash-ne$e.Value){throw 'Original installation/profile changed'}}elseif(Test-Path $e.Name){throw 'Previously absent original file appeared'}}
$backup=Get-Content "$root\system\winlogon-backup.json" -Raw|ConvertFrom-Json
$key=[Microsoft.Win32.Registry]::LocalMachine.OpenSubKey('SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon',$true)
if($key.GetValue('DefaultUserName')-ne$owned.account-or$key.GetValue('DefaultDomainName')-ne$env:COMPUTERNAME){throw 'Autologon ownership changed'}
$key.DeleteValue('DefaultPassword',$false);$key.SetValue('AutoAdminLogon','0');$key.Dispose()
Unregister-ScheduledTask 'Ottrin Window Input QA' -Confirm:$false
Add-Type @'
using System;using System.Runtime.InteropServices;
public static class OwnedLogoff {[DllImport("wtsapi32.dll",SetLastError=true)]public static extern bool WTSLogoffSession(IntPtr server,int session,bool wait);}
'@
$sessions=@(Get-CimInstance Win32_Process -Filter "Name='explorer.exe'"|Where-Object{(Invoke-CimMethod $_ -MethodName GetOwner).User-eq$owned.account}|Select-Object -ExpandProperty SessionId -Unique)
foreach($session in $sessions){if($session-le0-or-not[OwnedLogoff]::WTSLogoffSession([IntPtr]::Zero,$session,$true)){throw 'Owned console logoff failed'}}
$deadline=[DateTime]::UtcNow.AddSeconds(30)
do{$profiles=@(Get-CimInstance Win32_UserProfile|Where-Object{$_.SID-eq$owned.sid});if(-not@($profiles|Where-Object{$_.Loaded}).Count){break};Start-Sleep -Milliseconds 200}while([DateTime]::UtcNow-lt$deadline)
foreach($p in $profiles){if($p.Loaded-or$p.LocalPath-ne'C:\Users\OttrinWindowQA'){throw 'Wrong or loaded profile'};Remove-CimInstance $p};Remove-LocalUser $owned.account
$key=[Microsoft.Win32.Registry]::LocalMachine.OpenSubKey('SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon',$true)
foreach($e in $backup.PSObject.Properties){if($e.Value.present){$key.SetValue($e.Name,$e.Value.value,[Microsoft.Win32.RegistryValueKind][Enum]::Parse([Microsoft.Win32.RegistryValueKind],$e.Value.kind))}else{$key.DeleteValue($e.Name,$false)}}
if('DefaultPassword'-in$key.GetValueNames()){throw 'Temporary password remains'}
foreach($e in $backup.PSObject.Properties){if($e.Value.present){if($key.GetValue($e.Name,$null,[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)-ne$e.Value.value-or$key.GetValueKind($e.Name).ToString()-ne$e.Value.kind){throw 'Login restoration mismatch'}}elseif($e.Name-in$key.GetValueNames()){throw 'Previously absent login setting remains'}};$key.Dispose()
if(Get-LocalUser $owned.account -ErrorAction SilentlyContinue){throw 'Owned user remains'}
@{status='pass';ordinary_account_removed=$true;profile_removed=(-not(Test-Path 'C:\Users\OttrinWindowQA'));original_files_preserved=$true;login_values_restored=$true;temporary_password_removed=$true;actor_task_removed=$true}|ConvertTo-Json|Set-Content "$root\cleanup-result.json"
