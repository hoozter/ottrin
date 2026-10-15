import pathlib,sys,secrets,time,json,hashlib
root=pathlib.Path.cwd();sys.path.insert(0,str(root/'.hermes/evidence/windows-package-current'))
import native_compiler as n
n.GROOT=r'C:\OttrinDesktopQA'
for attempt in range(60):
 try:n.qga('guest-ping');break
 except RuntimeError:
  if attempt==59:raise
  time.sleep(1)
baseline=root/'dist/Ottrin-0.1.0-windows-x64.msi'
assert hashlib.sha256(baseline.read_bytes()).hexdigest()=='de47dae2de001294809187d91cb981d1e3b8aa6a542f5369045453b5807421f9'
expected={'artifacts':{'baseline.msi':hashlib.sha256(baseline.read_bytes()).hexdigest()}}
assert n.execute(r'''$root='C:\OttrinDesktopQA'
if(Test-Path $root){throw 'Unexpected existing QA root'}
if(Get-LocalUser -Name OttrinDesktopQA -ErrorAction SilentlyContinue){throw 'QA account exists'}
$key=[Microsoft.Win32.Registry]::LocalMachine.OpenSubKey('SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon',$false)
if('DefaultPassword' -in $key.GetValueNames()){throw 'Existing autologon credential; do not overwrite'}
$backup=@{}
foreach($name in @('DefaultUserName','DefaultDomainName','AutoAdminLogon')){if($name -in $key.GetValueNames()){$backup[$name]=@{present=$true;value=$key.GetValue($name);kind=$key.GetValueKind($name).ToString()}}else{$backup[$name]=@{present=$false}}}
$key.Dispose()
New-Item -ItemType Directory $root|Out-Null
$acl=New-Object System.Security.AccessControl.DirectorySecurity;$acl.SetAccessRuleProtection($true,$false)
foreach($sid in @('S-1-5-18','S-1-5-32-544')){$acl.AddAccessRule((New-Object System.Security.AccessControl.FileSystemAccessRule((New-Object System.Security.Principal.SecurityIdentifier($sid)),'FullControl','ContainerInherit,ObjectInherit','None','Allow')))}
Set-Acl $root $acl
$backup|ConvertTo-Json -Depth 6|Set-Content "$root\winlogon-backup.json"
@{preference_hash=(Get-FileHash 'C:\Users\campbell\AppData\Roaming\ottrin\config.json').Hash;prior_uninstaller_hash=(Get-FileHash 'C:\Users\campbell\AppData\Local\Programs\Ottrin\Uninstall.exe').Hash}|ConvertTo-Json|Set-Content "$root\baseline.json"
''')==0
for path,name in [(baseline,'baseline.msi'),(root/'.hermes/evidence/address-selection-current/VALID_CONFIG_TEMPLATE.json','config-template.json')]:n.upload(path,name)
# The randomly generated password exists only in this process and the temporary
# owned account/autologon registry. It is never logged or written to evidence.
password=secrets.token_urlsafe(28)+'Aa1!'
script=r'''$root='C:\OttrinDesktopQA';$name='OttrinDesktopQA'
$password=ConvertTo-SecureString '__PASSWORD__' -AsPlainText -Force
$user=New-LocalUser -Name $name -Password $password -Description 'Ottrin owned disposable desktop QA' -PasswordNeverExpires
$sid=$user.SID.Value
@{name=$name;sid=$sid;description=$user.Description}|ConvertTo-Json|Set-Content "$root\owned-account.json"
$acl=Get-Acl $root;$acl.AddAccessRule((New-Object System.Security.AccessControl.FileSystemAccessRule($user.SID,'ReadAndExecute','ContainerInherit,ObjectInherit','None','Allow')));Set-Acl $root $acl
New-Item -ItemType Directory "$root\results"|Out-Null
$acl=Get-Acl "$root\results";$acl.AddAccessRule((New-Object System.Security.AccessControl.FileSystemAccessRule($user.SID,'Modify','ContainerInherit,ObjectInherit','None','Allow')));Set-Acl "$root\results" $acl
$key=[Microsoft.Win32.Registry]::LocalMachine.OpenSubKey('SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon',$true)
if('DefaultPassword' -in $key.GetValueNames()){throw 'Autologon state changed during preparation'}
$key.SetValue('DefaultUserName',$name);$key.SetValue('DefaultDomainName',$env:COMPUTERNAME);$key.SetValue('DefaultPassword','__PASSWORD__');$key.SetValue('AutoAdminLogon','1');$key.Dispose()
Write-Output 'Owned ordinary account prepared; nonsecret Winlogon backup retained.'
'''.replace('__PASSWORD__',password)
assert n.execute(script)==0
(root/'.hermes/evidence/windows-upgrade-current/OWNERSHIP.json').write_text(json.dumps({'domain':'ottrin-index-qa','initial_state':'shut off','guest_root':n.GROOT,'account':'OttrinDesktopQA','purpose':'Native installer-controller and running-daemon upgrade acceptance','restore':'Exact nonsecret Winlogon values, remove owned password/account/profile/task/root; preserve campbell profile/installation; shut off domain','candidates':expected['artifacts']},indent=2)+'\n')
print('Ready for owned-account reboot.',flush=True)
