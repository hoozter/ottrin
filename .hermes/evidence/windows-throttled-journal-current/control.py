from pathlib import Path
import json,os,subprocess,sys,time,signal,xml.etree.ElementTree as E
root=Path.cwd();p=Path(os.environ.get('OTTRIN_NATIVE_EVIDENCE_DIR',str(Path(__file__).resolve().parent)))
sys.path.insert(0,str(root/'.hermes/evidence/windows-copy-cleanup-current'));import transport as n
owner=json.loads((p/'OWNERSHIP.json').read_text());domain=owner['domain'];target=owner['target']
def virsh(*args):return subprocess.check_output(['virsh',*args],text=True)
def guard():
 xml=E.fromstring(virsh('dumpxml',domain))
 disks=[disk for disk in xml.findall('./devices/disk') if disk.find('target') is not None and disk.find('target').get('dev')==target]
 assert len(disks)==1
 assert disks[0].find('source').get('file')=='/var/lib/libvirt/images/'+owner['secondary_volume']
 assert disks[0].find('serial').text==owner['secondary_serial']
 assert not xml.findall('./devices/interface')

def release():
 guard();virsh('blkdeviotune',domain,target,'--write-bytes-sec','0','--live')
def wait(script,label,seconds=45):
 pid=n.start(script);(p/(label+'.pid')).write_text(str(pid));n.wait(pid,seconds)
def interrupted(sig,frame):raise RuntimeError('Controller interrupted '+str(sig))
signal.signal(signal.SIGTERM,interrupted);signal.signal(signal.SIGINT,interrupted)
guard();timer=None;read_fd=write_fd=None;phase={'status':'preparing'}
try:
 wait(r"$deadline=(Get-Date).AddSeconds(40);while(-not(Test-Path C:\OttrinCopyCleanupQA\io-control\ready)){if((Get-Date)-gt$deadline){throw 'Test never reached append barrier'};Start-Sleep -Milliseconds 100};Get-Content C:\OttrinCopyCleanupQA\io-control\ready -Raw",'ready')
 phase['before_stats']=virsh('domblkstat',domain,target)
 phase['before_tune']=virsh('blkdeviotune',domain,target,'--live')
 virsh('blkdeviotune',domain,target,'--write-bytes-sec','1','--live')
 phase['active_tune']=virsh('blkdeviotune',domain,target,'--live')
 # Independent failsafe releases the disk even if the controller exits abruptly.
 read_fd,write_fd=os.pipe()
 code="import select,subprocess,sys;select.select([int(sys.argv[1])],[],[],10);subprocess.run(['virsh','blkdeviotune',sys.argv[2],sys.argv[3],'--write-bytes-sec','0','--live'],check=True)"
 timer=subprocess.Popen([sys.executable,'-c',code,str(read_fd),domain,target],pass_fds=(read_fd,),stdout=subprocess.DEVNULL)
 os.close(read_fd);read_fd=None
 wait(r"'go'|Set-Content C:\OttrinCopyCleanupQA\io-control\go;$deadline=(Get-Date).AddSeconds(8);while(-not(Test-Path C:\OttrinCopyCleanupQA\io-control\cancel-observed)){if((Get-Date)-gt$deadline){throw 'Cancellation observation missing'};Start-Sleep -Milliseconds 50};Get-Content C:\OttrinCopyCleanupQA\io-control\cancel-observed -Raw",'cancel',15)
 phase['after_stats']=virsh('domblkstat',domain,target);phase['status']='observed'
finally:
 release()
 if write_fd is not None:os.close(write_fd)
 if timer is not None:assert timer.wait(timeout=15)==0
 phase['released_tune']=virsh('blkdeviotune',domain,target,'--live')
 wait(r"'released'|Set-Content C:\OttrinCopyCleanupQA\io-control\released",'released',15)
 (p/'THROTTLE.json').write_text(json.dumps(phase,indent=2)+'\n')
