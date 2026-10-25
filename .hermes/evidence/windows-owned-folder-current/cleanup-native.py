from pathlib import Path
import sys,json,subprocess,time,hashlib
r=Path(__file__).resolve().parent;sys.path.insert(0,str(r));import transport as n
v=n.execute(r'''$ErrorActionPreference='Stop'
$root='C:\OttrinOwnedFolderQA'
$owner=Get-Content "$root\OWNER.json" -Raw|ConvertFrom-Json
$base=Get-Content "$root\BASELINE.json" -Raw|ConvertFrom-Json
$user=Get-LocalUser OttrinOwnedFolderQA
if($owner.sid-ne$user.SID.Value){throw 'Owned account identity changed'}
if($base.originalUsers.SID.Value-contains$user.SID.Value){throw 'Account was in original baseline'}
$task=Get-ScheduledTask -TaskName 'Ottrin Owned Folder QA' -ErrorAction SilentlyContinue
if($task-and$task.State-eq'Running'){throw 'Owned task remains live; observe before cleanup'}
$running=@(Get-CimInstance Win32_Process|Where-Object {$_.ProcessId-ne$PID-and(($_.ExecutablePath-and$_.ExecutablePath.StartsWith($root+'\',[StringComparison]::OrdinalIgnoreCase))-or($_.CommandLine-and$_.CommandLine.Contains($root)))})
if($running.Count){throw 'Owned native process remains live'}
$profile=Get-CimInstance Win32_UserProfile|Where-Object SID -eq $user.SID.Value
if($profile-and$profile.Loaded){throw 'Owned profile remains loaded; preserve before cleanup'}
if($task){Unregister-ScheduledTask -TaskName 'Ottrin Owned Folder QA' -Confirm:$false}
if($profile){Remove-CimInstance -InputObject $profile}
if(Get-CimInstance Win32_UserProfile|Where-Object SID -eq $user.SID.Value){throw 'Owned profile remains'}
Remove-LocalUser -SID $user.SID
if(Get-LocalUser OttrinOwnedFolderQA -ErrorAction SilentlyContinue){throw 'Owned account remains'}
$profiles=@(Get-CimInstance Win32_UserProfile|Select-Object SID,LocalPath)
foreach($old in $base.originalProfiles){
 if(-not($profiles|Where-Object {$_.SID-eq$old.SID-and$_.LocalPath-eq$old.LocalPath})){throw 'Original profile binding differs'}
}
$users=@(Get-LocalUser|Select-Object Name,SID)
foreach($old in $base.originalUsers){
 if(-not($users|Where-Object {$_.Name-eq$old.Name-and$_.SID.Value-eq$old.SID.Value})){throw 'Original user binding differs'}
}
$disks=@(Get-Disk|Select-Object Number,UniqueId,Size)
if(($disks|ConvertTo-Json -Depth 5 -Compress)-ne($base.disks|ConvertTo-Json -Depth 5 -Compress)){throw 'Original disk inventory differs'}
Remove-Item -LiteralPath $root -Recurse -Force
if(Test-Path -LiteralPath $root){throw 'Owned fixture remains'}
@{status='pass';task_removed=$true;account_removed=$true;profile_removed=$true;payload_and_fixture_removed=$true;original_profile_bindings_preserved=$true;original_user_bindings_preserved=$true;original_disks_preserved=$true;native_processes_absent=$true}|ConvertTo-Json
''')
print(v['out-data']);result=json.loads(v['out-data']);(r/'TEARDOWN_GUEST.json').write_text(json.dumps(result,indent=2)+'\n')
try:n.qga('guest-shutdown',{'mode':'powerdown'})
except RuntimeError as e:
 assert 'disappeared' in str(e) or 'not responding' in str(e),str(e)
for _ in range(80):
 state=subprocess.check_output(['virsh','-c','qemu:///system','domstate',n.guest.DOMAIN],text=True).strip()
 if state=='shut off':break
 time.sleep(.5)
assert state=='shut off',state
before=(r/'DOMAIN_BASELINE.xml').read_bytes();after=subprocess.check_output(['virsh','-c','qemu:///system','dumpxml','--inactive',n.guest.DOMAIN]);assert before==after,'Retained domain definition differs'
(r/'TEARDOWN.json').write_text(json.dumps({'status':'pass','retained_guest':'off','inactive_definition_unchanged':True,'definition_sha256':hashlib.sha256(before).hexdigest(),'owned_guest_fixture':'removed, TEARDOWN_GUEST.json','extra_disks_or_new_guests':'none','large_staging_remaining':'none; current Windows candidate remains retained deliverable'},indent=2)+'\n');(r/'DOMAIN_BASELINE.xml').unlink()
print('Retained guest off; definition unchanged; native fixture removed.')
