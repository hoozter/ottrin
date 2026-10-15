from pathlib import Path
import sys,base64,json
sys.path.insert(0,str(Path('.hermes/evidence/windows-20261011').resolve()))
from guest_exec import qga,execute
out=Path('.hermes/evidence/windows-commit-current')
def pull(name):
 h=qga('guest-file-open',{'path':'C:\\OttrinQA-20261011\\commit-current\\'+name,'mode':'rb'})
 try:
  data=bytearray()
  while True:
   r=qga('guest-file-read',{'handle':h,'count':65536});data.extend(base64.b64decode(r.get('buf-b64','')))
   if r.get('eof'):break
 finally:qga('guest-file-close',{'handle':h})
 value=json.loads(data.decode('utf-8-sig'));(out/name).write_text(json.dumps(value,indent=2)+'\n');return value
r=pull('RESULTS.json');t=pull('TASK_RESULTS.json');u=pull('USER_CLEANUP.json')
assert r['status']==t['status']==u['status']=='pass'
assert len(r['cases'])==11 and len(t['cases'])==2
b=json.loads((out/'BUILD.json').read_text())
for label,sha in b['artifacts'].items():assert u['source_artifacts'][label+'.exe']==sha
assert u['source_artifacts']['payload-transaction.qa.ps1']==b['fixture_sha256']['payload-transaction.qa.ps1']
assert execute(r'''
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$r='C:\OttrinQA-20261011\commit-current'
foreach($name in @('OttrinQA-CommitCurrent','OttrinQA-CommitTask')){
 if((Get-ScheduledTask $name).State -ne 'Ready'){throw 'QA task still live'}
}
if(Get-ScheduledTask|Where-Object TaskName -like 'Ottrin Commit QA *'){throw 'Disposable index task remains'}
$live=@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath -like "$r\*" -or $_.CommandLine -like '*OTTRIN_INSTALLER_PAYLOAD_SCRIPT*' -or $_.CommandLine -like '*-File C:\OttrinQA-20261011\commit-task.ps1*'})
if($live.Count){throw 'Fixture process remains'}
foreach($name in @('OttrinQA-CommitCurrent','OttrinQA-CommitTask')){Unregister-ScheduledTask $name -Confirm:$false}
Remove-Item $r -Recurse -Force
Remove-Item 'C:\OttrinQA-20261011\commit-task.ps1'
@{status='pass';fixture_tasks_and_processes_absent=$true;fixture_root_and_uploads_removed=$true;scope='Exact commit-current fixture only; existing installation and Office corpus preserved'}|ConvertTo-Json
''')==0
(out/'GUEST_CLEANUP.json').write_text(json.dumps({'status':'pass','fixture_tasks_processes_uploads_and_root_removed':True,'user_cleanup':'USER_CLEANUP.json'},indent=2)+'\n')
print('Collected 11 native installer cases and two real scheduled-task cases; removed owned guest fixtures')
