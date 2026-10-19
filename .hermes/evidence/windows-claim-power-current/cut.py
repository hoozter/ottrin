from pathlib import Path
import os
import json,subprocess,sys,time
sys.path.insert(0,str(Path('.hermes/evidence/windows-copy-cleanup-current').resolve()))
import transport as n
b=Path(__file__).resolve().parent
p=Path(os.environ.get("OTTRIN_NATIVE_EVIDENCE_DIR",str(b))).resolve();p.mkdir(parents=True,exist_ok=True);x=json.loads((p/'DISPATCH.json').read_text());assert x['status']=='dispatched'
pid=n.start(r"$deadline=(Get-Date).AddSeconds(30);$r=$null;while($null-eq$r){if((Get-Date)-gt$deadline){throw 'No current ready barrier'};if(Test-Path C:\OttrinCopyCleanupQA\results\POWER_READY.json){$v=Get-Content C:\OttrinCopyCleanupQA\results\POWER_READY.json -Raw|ConvertFrom-Json;if($v.dispatch_id-eq'TOKEN'){$r=$v}};if($null-eq$r){Start-Sleep -Milliseconds 50}};if($r.dispatch_id-ne'TOKEN'-or$r.status-ne'ready-for-cut'-or-not$r.ordinary_user-or($r.canonical_exists-ne$EXPECTED)){throw 'Wrong barrier'};$c=Get-CimInstance Win32_Process -Filter ('ProcessId='+$r.child_pid);if(-not$c-or$c.ExecutablePath-ne'C:\OttrinCopyCleanupQA\power-platform.exe'-or((Get-Date)-$c.CreationDate).TotalSeconds-gt60){throw 'Barrier process missing or too old; do not cut'};if((Get-ScheduledTask 'Ottrin Copy Cleanup QA').State.ToString()-ne'Running'){throw 'Actor stopped'};$a=(Get-FileHash C:\Users\campbell\AppData\Roaming\ottrin\config.json).Hash;$b=(Get-FileHash C:\Users\campbell\AppData\Local\Programs\Ottrin\Uninstall.exe).Hash;if($a-ne'5DE3C3B0125B564052E1546B34395558BF0F70F1F881DABE2F8A3289FA81371D'-or$b-ne'DDD1178F7C7391D9B344B2A9EC25AED1E82188610D28D340823473D6D21F0EC3'){throw 'Original profile/install changed'};$os=Get-CimInstance Win32_OperatingSystem;@{dispatch_id=$r.dispatch_id;child_live=$true;child_pid=$r.child_pid;profile_sha256=$a;install_sha256=$b;os_version=$os.Version;os_build=$os.BuildNumber}|ConvertTo-Json|Set-Content C:\OttrinCopyCleanupQA\results\POWER_LIVE.json".replace('TOKEN',x['id']).replace('EXPECTED','true' if x['phase']=='Persisted' else 'false'))
(p/'live.pid').write_text(str(pid));n.wait(pid,45)
n.download(r'results\POWER_READY.json',p/'BEFORE.json');n.download(r'results\POWER_LIVE.json',p/'LIVE.json')
r=json.loads((p/'BEFORE.json').read_text(encoding='utf-8-sig'));assert r['dispatch_id']==x['id'] and r['binary_sha256']==x['binary_sha256'] and r['root']==x['root']
disks=subprocess.check_output(['virsh','domblklist','ottrin-copy-cleanup-qa'],text=True);assert '/var/lib/libvirt/images/ottrin-copy-cleanup-qa.qcow2' in disks and 'vdb' not in disks
assert subprocess.check_output(['virsh','domstate','ottrin-copy-cleanup-qa'],text=True).strip()=='running'
subprocess.run(['virsh','destroy','ottrin-copy-cleanup-qa'],check=True)
state=subprocess.check_output(['virsh','domstate','ottrin-copy-cleanup-qa'],text=True).strip();assert state=='shut off'
(p/'CUT.json').write_text(json.dumps({'dispatch_id':x['id'],'method':'virsh destroy on isolated COW guest; no guest shutdown or handle cleanup','guest_state':state,'host_unix_time':time.time(),'scope':'Ungraceful guest power simulation; host and storage remained running'},indent=2)+'\n')
x['status']='cut';(p/'DISPATCH.json').write_text(json.dumps(x,indent=2)+'\n')
print('Cut completed at live '+x['phase']+' barrier',flush=True)
