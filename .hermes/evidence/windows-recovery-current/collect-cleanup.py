from pathlib import Path
import sys,base64,json,hashlib
sys.path.insert(0,str(Path('.hermes/evidence/windows-20261011').resolve()))
from guest_exec import qga,execute
out=Path('.hermes/evidence/windows-recovery-current')
def pull(name,source=None,encoding='utf-8-sig'):
 h=qga('guest-file-open',{'path':source or 'C:\\OttrinQA-20261011\\recovery-current\\'+name,'mode':'rb'})
 try:
  data=bytearray()
  while True:
   r=qga('guest-file-read',{'handle':h,'count':65536});data.extend(base64.b64decode(r.get('buf-b64','')))
   if r.get('eof'):break
 finally:qga('guest-file-close',{'handle':h})
 text=data.decode(encoding)
 if name.endswith('.json'):
  value=json.loads(text);(out/name).write_text(json.dumps(value,indent=2)+'\n');return value
 (out/name).write_text(text.replace('\r\n','\n'))
r=pull('RESULTS.json');u=pull('USER_CLEANUP.json');pull('RESET_REMAINING.json')
expected={'staging','publishing','old-moved','candidate-moved','published','integrating','rollback-candidate-moved','rollback-old-restored','rolled-back','committed','cleanup-leaf','cleanup-tree','fresh-published','fresh-committed','foreign-target','foreign-staged','torn-journal','missing-snapshot','root-extra','final-receipt','final-empty','foreign-completed'}
assert r['status']==u['status']=='pass'
assert len(r['cases'])==22 and {v['case'] for v in r['cases']}==expected
b=json.loads((out/'BUILD.json').read_text())
for name,sha in b['artifacts'].items():assert u['artifacts'][name]==sha
for name in ['actor.ps1','payload-transaction.qa.ps1']:assert u['artifacts'][name]==b['fixture_sha256'][name]

for p in out.iterdir():
 if p.suffix in ['.ps1','.py','.nsi','.c']:b['fixture_sha256'][p.name]=hashlib.sha256(p.read_bytes()).hexdigest()
(out/'BUILD.json').write_text(json.dumps(b,indent=2)+'\n')
for phase in ['RecoveryPlan','Initialize','Publish','BeginIntegration','Rollback','Commit','FinishRollback','SealCommit']:
 pull('LAST_'+phase+'.log','C:\\OttrinQA-20261011\\recovery-current\\phase-'+phase+'.txt',encoding='utf-16le')
for name in ['INITIAL_FAILURE.txt','SHORTCUT_FAILURE.txt','TIMEOUT_FAILURE.txt','RESUME_FAILURE.txt']:
 pull(name)
assert execute(r'''
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$r='C:\OttrinQA-20261011\recovery-current'
if((Get-ScheduledTask OttrinQA-RecoveryCurrent).State -ne 'Ready'){throw 'Fixture task is still running'}
$live=@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath -like "$r\*" -or $_.CommandLine -like '*OTTRIN_INSTALLER_PAYLOAD_SCRIPT*' -or $_.CommandLine -like '*-File C:\OttrinQA-20261011\recovery-*'})
if($live.Count){throw 'Fixture processes remain'}
Unregister-ScheduledTask OttrinQA-RecoveryCurrent -Confirm:$false
Remove-Item $r -Recurse -Force
foreach($name in @('recovery-actor.ps1','recovery-cleanup-user.ps1','recovery-reset.ps1','recovery-reset-remaining.ps1','recovery-reset-all.ps1','recovery-diagnose.ps1','recovery-diagnostic.exe','phase-debug.txt','phase-RecoveryPlan.txt','phase-Initialize.txt','phase-Publish.txt','phase-BeginIntegration.txt','phase-Rollback.txt','phase-Commit.txt','phase-FinishRollback.txt','phase-SealCommit.txt')){
 $p=Join-Path 'C:\OttrinQA-20261011' $name
 if(Test-Path -LiteralPath $p){Remove-Item -LiteralPath $p -Force}
}
@{status='pass';fixture_task_and_processes_absent=$true;fixture_root_and_uploads_removed=$true;scope='Exact recovery-current task, data, scripts and diagnostic outputs only; existing installation, profile and Office corpus retained'}|ConvertTo-Json
''')==0
(out/'GUEST_CLEANUP.json').write_text(json.dumps({'status':'pass','fixture_task_processes_uploads_and_root_removed':True,'user_cleanup':'USER_CLEANUP.json'},indent=2)+'\n')
print('Collected 22 native interruption/refusal cases; removed owned guest fixtures')
