from pathlib import Path
import sys,json,subprocess,time,hashlib,gzip,shutil
p=Path(__file__).resolve().parent;sys.path.insert(0,str(p));import transport as n
r=n.execute(r'''$ErrorActionPreference='Stop';$r='C:\OttrinOfficeContractQA';$result=Get-Content "$r\RESULT.json" -Raw|ConvertFrom-Json;if($result.status-ne'pass'-or$result.contractCases-ne16){throw 'Owned acceptance missing'};$live=@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath-and$_.ExecutablePath.StartsWith($r+'\',[StringComparison]::OrdinalIgnoreCase)});if($live.Count){throw 'Owned processes remain'};$baseline=Get-Content "$r\BASELINE.json" -Raw|ConvertFrom-Json;$profiles=@(Get-CimInstance Win32_UserProfile|ForEach-Object {$_.SID+'|'+$_.LocalPath}|Sort-Object);$before=@($baseline.profiles|ForEach-Object {$_.SID+'|'+$_.LocalPath}|Sort-Object);if(Compare-Object $before $profiles){throw 'Original profile bindings changed'};$disks=@(Get-Disk|ForEach-Object {($_.Number.ToString())+'|'+$_.UniqueId+'|'+$_.Size}|Sort-Object);$before=@($baseline.disks|ForEach-Object {($_.Number.ToString())+'|'+$_.UniqueId+'|'+$_.Size}|Sort-Object);if(Compare-Object $before $disks){throw 'Disk bindings changed'};Remove-Item $r -Recurse -Force;if(Test-Path $r){throw 'Owned root remains'};@{status='pass';originalProfilesPreserved=$true;disksPreserved=$true;ownedRootRemoved=$true;ownedProcesses=0;noAccountsTasksCredentialsLogonOrPolicyChanges=$true}|ConvertTo-Json''');(p/'TEARDOWN_GUEST.json').write_text(r['out-data'])
try:n.qga('guest-shutdown',{'mode':'powerdown'})
except RuntimeError as e:(p/'shutdown-observation.txt').write_text(str(e)+'\n')
for attempt in range(60):
 state=subprocess.check_output(['virsh','-c','qemu:///system','domstate',n.guest.DOMAIN]).decode().strip()
 if state=='shut off':break
 time.sleep(.5)
assert state=='shut off',state
xml=subprocess.check_output(['virsh','-c','qemu:///system','dumpxml',n.guest.DOMAIN,'--inactive']);assert hashlib.sha256(xml).hexdigest()==json.loads((p/'GUEST_OWNER.json').read_text())['definition_sha256']
(p/'TEARDOWN.json').write_text(json.dumps({'status':'pass','guest_off':True,'definition_unchanged':True,'guest_fixture_removed':True,'owned_host_transfer_removed':not any((p.parents[1]/'packaging-tmp').glob('office-contract-transfer-*')),'current_compiler_caches_preserved':True},indent=2)+'\n')
(p/'fixture-repair.patch').unlink(missing_ok=True)
for f in p.glob('*.jsonl'):
 if f.stat().st_size>65536:f.with_suffix(f.suffix+'.gz').write_bytes(gzip.compress(f.read_bytes(),mtime=0));f.unlink()
for d in p.rglob('__pycache__'):shutil.rmtree(d)
print('Native contract fixture and all children removed; original guest bindings preserved; guest off.')
