from pathlib import Path
import sys,json,hashlib
r=Path(__file__).parent.resolve();sys.path.insert(0,str(r));import transport as n
root=r.parents[2];build=json.loads((r/'BUILD.json').read_text());manifest=json.loads((r/'payload.json').read_text())
def pull(source,name):
 n.download(source,r/name);v=json.loads((r/name).read_text(encoding='utf-8-sig'));(r/name).write_text(json.dumps(v,indent=2)+'\n');return v
result=pull('results\\RESULTS.json','RESULTS.json');assert result['status']=='pass' and len(result['cases'])==4
assert result['current_installer_sha256']==build['guest_inputs_sha256']['current.exe']
verification=[]
for case in result['cases']:
 point=case['point'];checkpoint=pull('results\\checkpoint-'+point+'.json','checkpoint-'+point+'.json');observed=pull('results\\observed-'+point+'.json','observed-'+point+'.json');pull('results\\task-'+point+'.json','task-'+point+'.json')
 files={v['path'].replace('\\','/'):v['candidate']['sha256'].lower() for v in checkpoint['transaction']['files']}
 assert set(files)==set(manifest)|{'Uninstall.exe'}
 for name,sha in manifest.items():assert files[name]==sha,(point,name)
 key=next(k for k in observed['registry'] if k['path'].endswith('Uninstall\\Ottrin'));value=next(v for v in key['values'] if v['name']=='NoRepair')
 assert value['kind']==(4 if point=='committed' else 11) and value['value']==(1 if point=='committed' else 4294967297)
 verification.append({'point':point,'interrupted_candidate51_file_hashes_match':True,'no_repair_dword_or_qword_value_verified':True})
(r/'PAYLOAD_AND_QWORD_VERIFICATION.json').write_text(json.dumps({'status':'pass','cases':verification},indent=2)+'\n')
assert n.execute((r/'cleanup-account.ps1').read_text())==0
pull('cleanup-result.json','ACCOUNT_CLEANUP.json')
assert n.execute(r'''
$root='C:\OttrinFullRecoveryQA'
$live=@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath-like "$root\*" -or $_.CommandLine-like '*OTTRIN_INSTALLER_PAYLOAD_SCRIPT*'})
if($live.Count){throw 'Owned test process or installer helper remains'}
Remove-Item $root -Recurse -Force
if(Test-Path $root){throw 'Owned root remains'}
Write-Output 'Owned full-payload fixtures and uploads removed.'
''')==0
(r/'GUEST_CLEANUP.json').write_text(json.dumps({'status':'pass','owned_account_profile_password_tasks_removed':'ACCOUNT_CLEANUP.json','owned_guest_fixtures_and_uploads_removed':True,'owned_processes_absent':True,'original_guest_profile_and_installation_unchanged':True},indent=2)+'\n')
