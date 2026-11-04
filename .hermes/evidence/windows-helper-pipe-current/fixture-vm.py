"""Owned disposable native helper QA VM, with teardown registered before allocation."""
from pathlib import Path
import datetime,hashlib,json,signal,subprocess,time,uuid,xml.etree.ElementTree as ET
R=Path(__file__).resolve().parent
NAME='ottrin-init-helper-pipe-qa';VOLUME=NAME+'.qcow2';VM_UUID=str(uuid.uuid4())
PROTECTED=['ottrin-copy-cleanup-qa','ottrin-index-qa','ottrin-qa-20261011','win11ltsc']
def run(*args):return subprocess.check_output(['virsh','-c','qemu:///system',*args],text=True).strip()
def save(name,value):(R/name).write_text(json.dumps(value,indent=2)+'\n')
def cancelled(*_):raise KeyboardInterrupt('Owned VM controller cancelled')
for signum in [signal.SIGINT,signal.SIGTERM]:signal.signal(signum,cancelled)
assert NAME not in run('list','--all','--name').splitlines()
baseline={name:{'xml':hashlib.sha256(run('dumpxml',name,'--inactive').encode()).hexdigest(),'state':run('domstate',name)} for name in PROTECTED}
assert all(row['state']=='shut off' for row in baseline.values())
parent=run('dumpxml','ottrin-qa-20261011','--inactive');tree=ET.fromstring(parent)
backing=tree.find('./devices/disk/source').get('file');assert backing=='/var/lib/libvirt/images/ottrin-qa-20261011.qcow2'
backing_stat=Path(backing).stat();volume_created=False;defined=False
save('OWNERSHIP.json',{'domain':NAME,'uuid':VM_UUID,'volume':VOLUME,'backing':backing,'purpose':'Native shared pipe, packaged helper and UAC verification only','cleanup':'This live controller owns stopping/undefining the domain, NVRAM/TPM removal and exact overlay deletion','protected':baseline})
try:
 run('vol-create-as','default',VOLUME,'64G','--format','qcow2','--backing-vol',backing,'--backing-vol-format','qcow2');volume_created=True
 tree.find('name').text=NAME;tree.find('uuid').text=VM_UUID
 if tree.find('genid') is not None:tree.find('genid').text=str(uuid.uuid4())
 tree.find('./os/nvram').text='/var/lib/libvirt/qemu/nvram/'+NAME+'_VARS.fd'
 tree.find('./devices/disk/source').set('file','/var/lib/libvirt/images/'+VOLUME)
 for interface in tree.findall('./devices/interface'):tree.find('devices').remove(interface)
 path=R/'owned-domain.xml';ET.ElementTree(tree).write(path,encoding='unicode')
 run('define',str(path));defined=True;assert run('domuuid',NAME)==VM_UUID
 run('start',NAME);print('Owned native VM running:',NAME,flush=True)
 save('VM_READY.json',{'domain':NAME,'uuid':VM_UUID,'started_at':datetime.datetime.now(datetime.timezone.utc).isoformat()})
 deadline=time.monotonic()+7200
 while not (R/'STOP').exists():
  assert time.monotonic()<deadline,'Owned VM lifetime limit reached'
  time.sleep(1)
finally:
 cleanup={'domain':NAME,'uuid':VM_UUID,'volume':VOLUME,'status':'teardown started'}
 if defined:
  assert run('domuuid',NAME)==VM_UUID
  if run('domstate',NAME)!='shut off':run('destroy',NAME)
  assert not run('snapshot-list',NAME,'--name')
  run('undefine',NAME,'--nvram','--tpm');cleanup['domain_removed']=True
 if volume_created:
  assert NAME not in run('list','--all','--name').splitlines()
  run('vol-delete',VOLUME,'--pool','default');cleanup['overlay_removed']=True
 after=Path(backing).stat();assert (after.st_dev,after.st_ino,after.st_mtime_ns,after.st_size)==(backing_stat.st_dev,backing_stat.st_ino,backing_stat.st_mtime_ns,backing_stat.st_size)
 assert {name:{'xml':hashlib.sha256(run('dumpxml',name,'--inactive').encode()).hexdigest(),'state':run('domstate',name)} for name in PROTECTED}==baseline
 cleanup.update(status='pass',protected_definitions_states_and_backing_unchanged=True)
 save('VM_CLEANUP.json',cleanup)
 (R/'VM_READY.json').unlink(missing_ok=True);(R/'STOP').unlink(missing_ok=True);(R/'owned-domain.xml').unlink(missing_ok=True)
 print('Owned native VM and overlay removed',flush=True)
