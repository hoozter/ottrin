$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$root='C:\OttrinPackageQA';$owned=Get-Content "$root\owned-account.json" -Raw|ConvertFrom-Json
$user=Get-LocalUser -Name $owned.name
if($user.SID.Value-ne$owned.sid -or $user.Description-ne'Ottrin owned disposable package QA'){throw 'Wrong account ownership'}
$s=New-Object -ComObject Schedule.Service;$s.Connect();$folder=$s.GetFolder('\')
foreach($name in @('Ottrin Package QA')){$t=$folder.GetTask($name);if($t.State-eq4){throw "Owned actor still active: $name"}}
$taskAbsent=$false;try{$null=$folder.GetTask('Ottrin Index '+$owned.sid)}catch{if($_.Exception.HResult-eq-2147024894){$taskAbsent=$true}else{throw}}
if(-not$taskAbsent){throw 'Owned index task still registered'}
$profile='C:\Users\OttrinPackageQA'
if(Test-Path "$profile\AppData\Local\Programs\Ottrin"){throw 'Owned installation remains'}
if(@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath-like "$profile\AppData\Local\Programs\Ottrin\*"}).Count){throw 'Owned application process remains'}
if(@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath-like "$root\*"}).Count){throw 'Source probe process remains'}
$b=Get-Content "$root\baseline.json" -Raw|ConvertFrom-Json
if((Get-FileHash 'C:\Users\campbell\AppData\Roaming\ottrin\config.json').Hash-ne$b.preference_hash){throw 'Existing preference changed'}
if((Get-FileHash 'C:\Users\campbell\AppData\Local\Programs\Ottrin\Uninstall.exe').Hash-ne$b.prior_uninstaller_hash){throw 'Existing installation changed'}
$backup=Get-Content "$root\winlogon-backup.json" -Raw|ConvertFrom-Json
$key=[Microsoft.Win32.Registry]::LocalMachine.OpenSubKey('SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon',$true)
if($key.GetValue('DefaultUserName')-ne$owned.name -or $key.GetValue('DefaultDomainName')-ne$env:COMPUTERNAME){throw 'Unexpected current autologon owner'}
# Delete only the temporary credential; never read its value or an earlier one.
$key.DeleteValue('DefaultPassword',$false)
foreach($entry in $backup.PSObject.Properties){if($entry.Value.present){$kind=[Microsoft.Win32.RegistryValueKind][Enum]::Parse([Microsoft.Win32.RegistryValueKind],$entry.Value.kind);$key.SetValue($entry.Name,$entry.Value.value,$kind)}else{$key.DeleteValue($entry.Name,$false)}}
$key.Dispose()
foreach($name in @('Ottrin Package QA')){Unregister-ScheduledTask -TaskName $name -Confirm:$false}
Add-Type @'
using System;using System.Runtime.InteropServices;
public static class OwnedLogoff {
 [DllImport("wtsapi32.dll",SetLastError=true)] public static extern bool WTSLogoffSession(IntPtr server,int session,bool wait);
}
'@
$explorers=@(Get-CimInstance Win32_Process -Filter "Name='explorer.exe'"|Where-Object {(Invoke-CimMethod $_ -MethodName GetOwner).User-eq$owned.name})
$sessions=@($explorers|Select-Object -ExpandProperty SessionId -Unique)
foreach($session in $sessions){if($session-le0){throw 'Unexpected own session'};if(-not[OwnedLogoff]::WTSLogoffSession([IntPtr]::Zero,$session,$true)){throw "Owned session logoff failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"}}
$deadline=[DateTime]::UtcNow.AddSeconds(30)
do{$profiles=@(Get-CimInstance Win32_UserProfile|Where-Object {$_.SID-eq$owned.sid});if(-not@($profiles|Where-Object {$_.Loaded}).Count){break};Start-Sleep -Milliseconds 200}while([DateTime]::UtcNow-lt$deadline)
foreach($p in $profiles){if($p.Loaded -or $p.LocalPath-ne$profile){throw 'Owned profile still loaded or wrong path'};Remove-CimInstance $p}
Remove-LocalUser -Name $owned.name
if(Get-LocalUser -Name $owned.name -ErrorAction SilentlyContinue){throw 'Owned account remains'}
$key=[Microsoft.Win32.Registry]::LocalMachine.OpenSubKey('SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon',$false)
if('DefaultPassword'-in$key.GetValueNames()){throw 'Temporary autologon password remains'}
foreach($entry in $backup.PSObject.Properties){if($entry.Value.present){if($key.GetValue($entry.Name)-ne$entry.Value.value -or $key.GetValueKind($entry.Name).ToString()-ne$entry.Value.kind){throw 'Login settings not restored exactly'}}elseif($entry.Name-in$key.GetValueNames()){throw 'Previously absent login value remains'}}
$key.Dispose()
@{status='pass';domain='ottrin-qa-20261011';owned_account_removed=$true;owned_profile_removed=(-not(Test-Path $profile));owned_index_task_removed=$taskAbsent;actor_tasks_removed=$true;login_values_restored_exactly=$true;temporary_password_removed=$true;existing_preference_hash=$b.preference_hash;existing_installation_uninstaller_hash=$b.prior_uninstaller_hash}|ConvertTo-Json|Set-Content "$root\cleanup-result.json"
