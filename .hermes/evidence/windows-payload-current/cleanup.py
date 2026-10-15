from pathlib import Path
import sys,json,base64
sys.path.insert(0,str(Path('.hermes/evidence/windows-20261011').resolve()))
from guest_exec import qga,execute
root=Path('.hermes/evidence/windows-payload-current');build=json.loads((root/'BUILD.json').read_text())
h=qga('guest-file-open',{'path':r'C:\OttrinQA-20261011\payload-current\USER_CLEANUP.json','mode':'rb'})
try:
 data=bytearray()
 while True:
  r=qga('guest-file-read',{'handle':h,'count':65536});data.extend(base64.b64decode(r.get('buf-b64','')))
  if r.get('eof'):break
finally:qga('guest-file-close',{'handle':h})
u=json.loads(data.decode('utf-8-sig'));assert u['status']=='pass'
for label in ['baseline','fixed','staging-fault']:assert u['source_artifacts'][label+'.exe']==build['artifacts'][label]
assert u['source_artifacts']['payload-transaction.qa.ps1']==build['fixture_sha256']['payload-transaction.qa.ps1']
(root/'USER_CLEANUP.json').write_text(json.dumps(u,indent=2)+'\n')
script=r'''
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$r='C:\OttrinQA-20261011\payload-current'
if((Get-ScheduledTask OttrinQA-PayloadCurrent).State -ne 'Ready'){throw 'Fixture task is still live'}
$live=@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath -like "$r\*" -or $_.CommandLine -like '*OTTRIN_INSTALLER_PAYLOAD_SCRIPT*'})
if($live.Count){throw 'Fixture processes remain'}
Unregister-ScheduledTask OttrinQA-PayloadCurrent -Confirm:$false
Remove-Item $r -Recurse -Force
foreach($name in @('payload-actor.ps1','payload-reset.ps1','payload-diagnose.ps1','payload-leaf.ps1','payload-cleanup-user.ps1','payload-fixed-debug.exe','phase-debug.txt')){
 $p=Join-Path 'C:\OttrinQA-20261011' $name
 if(Test-Path -LiteralPath $p){Remove-Item -LiteralPath $p -Force}
}
@{status='pass';owned_task_removed=$true;owned_fixture_root_and_uploads_removed=$true;live_fixture_installers_and_payload_controllers=0;scope='Exact QA task, uploaded scripts/debug executable and task-owned fixture root only; original installation, profile and Office corpus retained'}|ConvertTo-Json
'''
assert execute(script)==0
