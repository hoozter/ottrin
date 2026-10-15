from pathlib import Path
import sys,secrets,json
sys.path.insert(0,str(Path(__file__).parent.resolve()));import transport as n
assert n.execute(r'''
$root='C:\OttrinFullRecoveryQA'
if(Get-LocalUser OttrinFullRecoveryQA -ErrorAction SilentlyContinue){throw 'Owned account exists'}
$key=[Microsoft.Win32.Registry]::LocalMachine.OpenSubKey('SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon',$false)
if('DefaultPassword' -in $key.GetValueNames()){throw 'An autologon credential already exists'}
$backup=@{};foreach($name in @('DefaultUserName','DefaultDomainName','AutoAdminLogon')){
 if($name -in $key.GetValueNames()){$backup[$name]=@{present=$true;value=$key.GetValue($name);kind=$key.GetValueKind($name).ToString()}}else{$backup[$name]=@{present=$false}}
};$key.Dispose()
$backup|ConvertTo-Json -Depth 6|Set-Content "$root\winlogon-backup.json"
''')==0
password=secrets.token_urlsafe(28)+'Aa1!'
script=r'''
try {
 $root='C:\OttrinFullRecoveryQA';$name='OttrinFullRecoveryQA'
 $password=ConvertTo-SecureString '__PASSWORD__' -AsPlainText -Force
 $user=New-LocalUser -Name $name -Password $password -Description 'Ottrin owned disposable full recovery QA' -PasswordNeverExpires
 @{name=$name;sid=$user.SID.Value;description=$user.Description}|ConvertTo-Json|Set-Content "$root\owned-account.json"
 $acl=Get-Acl $root;$acl.AddAccessRule((New-Object System.Security.AccessControl.FileSystemAccessRule($user.SID,'ReadAndExecute','ContainerInherit,ObjectInherit','None','Allow')));Set-Acl $root $acl
 New-Item -ItemType Directory "$root\results"|Out-Null
 $acl=Get-Acl "$root\results";$acl.AddAccessRule((New-Object System.Security.AccessControl.FileSystemAccessRule($user.SID,'Modify','ContainerInherit,ObjectInherit','None','Allow')));Set-Acl "$root\results" $acl
 $key=[Microsoft.Win32.Registry]::LocalMachine.OpenSubKey('SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon',$true)
 if('DefaultPassword' -in $key.GetValueNames()){throw 'Autologon owner changed'}
 $key.SetValue('DefaultUserName',$name);$key.SetValue('DefaultDomainName',$env:COMPUTERNAME);$key.SetValue('DefaultPassword','__PASSWORD__');$key.SetValue('AutoAdminLogon','1');$key.Dispose()
 Write-Output 'Disposable non-administrator interactive account prepared.'
} catch { [Console]::Error.Write('Owned account preparation failed: '+$_.Exception.GetType().FullName);exit 1 }
'''.replace('__PASSWORD__',password)
assert n.execute(script)==0
Path(__file__).with_name('OWNERSHIP.json').write_text(json.dumps({'domain':n.guest.DOMAIN,'guest_root':n.GROOT,'account':'OttrinFullRecoveryQA','purpose':'Current coherent Windows installer and real daemon/scheduler acceptance','restore':'Restore exact nonsecret Winlogon values; remove temporary credential/account/profile/tasks/root; preserve original campbell installation/profile; shut off guest'},indent=2)+'\n')
