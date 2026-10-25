from pathlib import Path
import sys,json,base64,subprocess,hashlib,time,gzip
import native_compiler as n
p=Path(__file__).parent
script=r'''$r='C:\OttrinCIPackageQA';$live=@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath -and $_.ExecutablePath.StartsWith($r+'\',[StringComparison]::OrdinalIgnoreCase)});if($live.Count){throw 'Owned processes remain; inspect before deleting'};if(-not(Test-Path "$r\MSI_PAYLOAD.json")){throw 'Owned receipt missing; root identity uncertain'};Remove-Item $r -Recurse -Force;if(Test-Path $r){throw 'Fixture removal failed'};@{status='pass';ownedRootRemoved=$true;ownedProcesses=0;scope='Only C:\OttrinCIPackageQA fixture; no package installation or guest configuration/profile changes performed'}|ConvertTo-Json'''
r=n.wait(n.start(script));(p/'TEARDOWN_GUEST.json').write_bytes(base64.b64decode(r['out-data']))
try:n.qga('guest-shutdown',{'mode':'powerdown'})
except RuntimeError as e:(p/'shutdown-observation.txt').write_text(str(e)+'\n')
for attempt in range(50):
 state=subprocess.check_output(['virsh','-c','qemu:///system','domstate',n.guest.DOMAIN]).decode().strip()
 if state=='shut off':break
 time.sleep(.5)
assert state=='shut off',state
xml=subprocess.check_output(['virsh','-c','qemu:///system','dumpxml',n.guest.DOMAIN,'--inactive']);before=json.loads((p/'GUEST_OWNER.json').read_text());assert hashlib.sha256(xml).hexdigest()==before['inactive_xml_sha256']
(p/'TEARDOWN.json').write_text(json.dumps({'status':'pass','domain':n.guest.DOMAIN,'final_state':state,'definition_unchanged':True,'guest_fixture_removed':True,'no_new_disks_accounts_or_guests':True,'stage_pending_consumer':'Retained host ZIP/NSIS/MSI/source stage for native GUI and full Windows CI acceptance; previous accepted candidate preserved'},indent=2)+'\n')
for f in p.glob('*.log'):
 if f.stat().st_size>65536:f.with_suffix(f.suffix+'.gz').write_bytes(gzip.compress(f.read_bytes(),mtime=0));f.unlink()
