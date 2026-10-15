from pathlib import Path
import sys
sys.path.insert(0,str(Path(__file__).parent.resolve()));import transport as n
assert n.execute(r'''
$root='C:\OttrinFullRecoveryQA'
if(Test-Path $root){throw 'Owned root already exists'}
if(Get-LocalUser OttrinFullRecoveryQA -ErrorAction SilentlyContinue){throw 'Owned account already exists'}
New-Item -ItemType Directory $root|Out-Null
$acl=New-Object Security.AccessControl.DirectorySecurity;$acl.SetAccessRuleProtection($true,$false)
foreach($sid in @('S-1-5-18','S-1-5-32-544')){$acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule((New-Object Security.Principal.SecurityIdentifier($sid)),'FullControl','ContainerInherit,ObjectInherit','None','Allow')))}
Set-Acl $root $acl
@{preference_hash=(Get-FileHash 'C:\Users\campbell\AppData\Roaming\ottrin\config.json').Hash;prior_uninstaller_hash=(Get-FileHash 'C:\Users\campbell\AppData\Local\Programs\Ottrin\Uninstall.exe').Hash}|ConvertTo-Json|Set-Content "$root\baseline.json"
''')==0
