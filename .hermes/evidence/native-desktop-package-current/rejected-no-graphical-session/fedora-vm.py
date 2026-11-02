"""Owned fresh Fedora desktop/package lifecycle, with registered libvirt teardown."""
from pathlib import Path
from contextlib import ExitStack
import hashlib,json,os,shlex,signal,subprocess,tempfile,time,uuid,xml.etree.ElementTree as ET
r=Path.cwd();e=Path(__file__).resolve().parent
base=r/'.hermes/tools/fedora44/Fedora-Cloud-Base-Generic-44-1.7.x86_64.qcow2'
assert hashlib.file_digest(base.open('rb'),'sha256').hexdigest()=='28680fe5b371a5a82ebf43a31926e086a168e59949d03969c5093e7071f90b7f'
build=json.loads((e/'BUILD_RESULT.json').read_text());assert build['status'].startswith('four-format build pass')
stage=Path(build['stage']);rpm=next(stage.glob('*.rpm'));portable=next(stage.glob('*portable.tar.gz'))
name='ottrin-native-desktop-qa';mac='52:54:00:6f:74:47';prefix=name+'-';owned_volumes=[];domain_uuid=None
report={'domain':name,'mac':mac,'source':build['source_revision'],'status':'running','rpm_sha256':hashlib.file_digest(rpm.open('rb'),'sha256').hexdigest()}
def run(args,check=True,timeout=180):
 p=subprocess.run(list(map(str,args)),capture_output=True,text=True,timeout=timeout)
 if check and p.returncode:raise RuntimeError(f'{args}: {p.returncode}: {p.stdout[-1500:]} {p.stderr[-1500:]}')
 return p
def virsh(*args,**kw):return run(['virsh','-c','qemu:///system',*args],**kw)
def cancelled(*_):raise KeyboardInterrupt('Owned native desktop VM cancelled')
for sig in [signal.SIGINT,signal.SIGTERM]:signal.signal(sig,cancelled)
assert name not in virsh('list','--all','--name').stdout.splitlines()
assert prefix not in virsh('vol-list','default','--details').stdout
def teardown():
 global domain_uuid
 cleanup={}
 if domain_uuid and name in virsh('list','--all','--name').stdout.splitlines():
  xml=ET.fromstring(virsh('dumpxml',name).stdout)
  assert xml.findtext('uuid')==domain_uuid
  assert xml.find('devices/interface/mac').attrib['address']==mac
  assert not virsh('snapshot-list',name,'--name').stdout.strip()
  disks={x.attrib['file'] for x in xml.findall('devices/disk/source') if 'file' in x.attrib}
  expected={virsh('vol-path',v,'--pool','default').stdout.strip() for v in owned_volumes[1:]}
  assert disks==expected,(disks,expected)
  if virsh('domstate',name).stdout.strip()!='shut off':virsh('destroy',name)
  virsh('undefine',name);cleanup['domain_removed']=True
 for volume in reversed(owned_volumes):
  # No retained guest may consume these owned files directly or through backing chains.
  path=virsh('vol-path',volume,'--pool','default').stdout.strip()
  for domain in virsh('list','--all','--name').stdout.splitlines():
   if domain:assert path not in virsh('dumpxml',domain).stdout
  virsh('vol-delete',volume,'--pool','default')
 cleanup['volumes_removed']=owned_volumes.copy();(e/'VM_CLEANUP.json').write_text(json.dumps(cleanup,indent=2)+'\n')
try:
 with tempfile.TemporaryDirectory(prefix='.native-vm-',dir=e) as temporary,ExitStack() as cleanup:
  temporary=Path(temporary);cleanup.callback(teardown)
  key=temporary/'id_ed25519';run(['ssh-keygen','-q','-t','ed25519','-N','','-f',key])
  public=key.with_suffix('.pub').read_text().strip()
  (temporary/'user-data').write_text('#cloud-config\nhostname: '+name+'\nssh_pwauth: false\nusers:\n  - name: ottrinqa\n    groups: wheel\n    sudo:\n      - ALL=(ALL) NOPASSWD:ALL\n    shell: /bin/bash\n    ssh_authorized_keys:\n      - '+public+'\n')
  (temporary/'meta-data').write_text('instance-id: '+name+'\nlocal-hostname: '+name+'\n')
  seed=temporary/'seed.iso';run(['genisoimage','-quiet','-output',seed,'-volid','cidata','-joliet','-rock',temporary/'user-data',temporary/'meta-data'])
  for suffix,capacity,fmt,backing in [('base.qcow2',5368709120,'qcow2',None),('work.qcow2',12884901888,'qcow2',prefix+'base.qcow2'),('seed.iso',seed.stat().st_size,'raw',None)]:
   volume=prefix+suffix;xml=ET.Element('volume');ET.SubElement(xml,'name').text=volume;ET.SubElement(xml,'capacity',unit='bytes').text=str(capacity);target=ET.SubElement(xml,'target');ET.SubElement(target,'format',type=fmt)
   if backing:
    store=ET.SubElement(xml,'backingStore');ET.SubElement(store,'path').text=virsh('vol-path',backing,'--pool','default').stdout.strip();ET.SubElement(store,'format',type='qcow2')
   source=temporary/(suffix+'.xml');source.write_text(ET.tostring(xml,encoding='unicode'))
   virsh('vol-create','default',source);owned_volumes.append(volume)
   if suffix!='work.qcow2':virsh('vol-upload',volume,base if suffix=='base.qcow2' else seed,'--pool','default',timeout=180)
  (e/'VM_OWNER.json').write_text(json.dumps({'domain':name,'mac':mac,'volumes':owned_volumes,'base_cache':str(base),'cleanup_owner':str(Path(__file__))},indent=2)+'\n')
  domain_uuid=str(uuid.uuid4())
  run(['virt-install','--connect','qemu:///system','--name',name,'--uuid',domain_uuid,'--memory','3072','--vcpus','2','--import','--disk','vol=default/'+prefix+'work.qcow2,bus=virtio','--disk','vol=default/'+prefix+'seed.iso,device=cdrom','--network','network=default,mac='+mac,'--graphics','none','--osinfo','generic','--noautoconsole'],timeout=60)
  assert virsh('domuuid',name).stdout.strip()==domain_uuid;report['uuid']=domain_uuid
  deadline=time.monotonic()+240;address=None
  while time.monotonic()<deadline:
   text=virsh('domifaddr',name,'--source','lease').stdout
   for line in text.splitlines():
    fields=line.split()
    if mac in fields and len(fields)>3:address=fields[3].split('/')[0]
   if address:
    ssh=['ssh','-F','/dev/null','-o','ForwardX11=no','-i',str(key),'-o','IdentitiesOnly=yes','-o','BatchMode=yes','-o','StrictHostKeyChecking=yes','-o','UserKnownHostsFile='+str(temporary/'known_hosts'),'-o','ConnectTimeout=4','ottrinqa@'+address]
    if not (temporary/'known_hosts').exists():
     scan=run(['ssh-keyscan','-T','3',address],check=False,timeout=6)
     if scan.returncode==0:(temporary/'known_hosts').write_text(scan.stdout)
    if (temporary/'known_hosts').exists() and run(ssh+['true'],check=False,timeout=8).returncode==0:break
   time.sleep(2)
  else:raise RuntimeError('Owned guest SSH unavailable')
  def remote(*args,check=True,timeout=180):return run(ssh+[shlex.join(list(map(str,args)))],check=check,timeout=timeout)
  cloud=remote('sudo','cloud-init','status','--wait','--format=json',check=False,timeout=180)
  (e/'VM_CLOUD_INIT.json').write_text(cloud.stdout)
  (e/'VM_CLOUD_INIT.stderr').write_text(cloud.stderr)
  cloud_state=json.loads(cloud.stdout)
  assert cloud.returncode in (0,2) and cloud_state['status']=='done' and not cloud_state.get('errors'),cloud_state
  report['cloud_init']={'exit':cloud.returncode,'status':cloud_state['status'],'recoverable_errors':cloud_state.get('recoverable_errors'),'scope':'Provisioning warnings retained separately; product gates unchanged'}
  scp=['scp','-F','/dev/null','-o','ForwardX11=no','-i',str(key),'-o','IdentitiesOnly=yes','-o','BatchMode=yes','-o','StrictHostKeyChecking=yes','-o','UserKnownHostsFile='+str(temporary/'known_hosts')]
  for source,destination in [(rpm,'candidate.rpm'),(portable,'portable.tar.gz'),(e/'fedora-actor.py','actor.py')]:run(scp+[str(source),'ottrinqa@'+address+':'+destination],timeout=180)
  install=remote('sudo','dnf','-y','install','/home/ottrinqa/candidate.rpm','xdg-desktop-portal','xdg-desktop-portal-gtk',check=False,timeout=600);(e/'VM_INSTALL.log').write_text(install.stdout+install.stderr)
  assert install.returncode==0,'Native package install failed; see VM_INSTALL.log'
  result=remote('sudo','python3','/home/ottrinqa/actor.py',build['source_revision'],check=False,timeout=600);(e/'VM_ACTOR.log').write_text(result.stdout+result.stderr)
  run(scp+['ottrinqa@'+address+':results.tar.gz',str(e/'VM_RESULTS.tar.gz')],timeout=60)
  assert result.returncode==0,result.stdout[-2000:]+result.stderr[-1000:]
  report['status']='pass within actor scope'
except BaseException as error:
 report.update(status='fail',error=repr(error));raise
finally:
 report['temporary_seed_and_ssh_keys_removed']=not any(e.glob('.native-vm-*'));(e/'VM_RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
