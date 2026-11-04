"""Owned fresh Linux desktop/package lifecycle, with registered libvirt teardown."""
from pathlib import Path
from contextlib import ExitStack
import argparse,hashlib,json,os,shlex,signal,subprocess,sys,tempfile,time,uuid,xml.etree.ElementTree as ET
r=Path.cwd();drivers=Path(__file__).resolve().parent
parser=argparse.ArgumentParser();parser.add_argument('distro',choices=['fedora','ubuntu']);parser.add_argument('--output',type=Path,required=True);parser.add_argument('--published',action='store_true');parser.add_argument('--actor',type=Path);parser.add_argument('--input',type=Path,action='append',default=[]);parser.add_argument('--gui-qa',action='store_true');args=parser.parse_args();e=args.output.resolve();assert e.is_relative_to(r/'.hermes/evidence');distro=args.distro
assert __import__('shutil').disk_usage('/var/lib/libvirt/images').free >= 20*1024**3, 'Need20GiB before allocating a desktop fixture'
base=r/('.hermes/tools/fedora44/Fedora-Cloud-Base-Generic-44-1.7.x86_64.qcow2' if distro=='fedora' else '.hermes/tools/ubuntu24/noble-server-cloudimg-amd64.img')
expected_base='28680fe5b371a5a82ebf43a31926e086a168e59949d03969c5093e7071f90b7f' if distro=='fedora' else '6a81c37564db9b1ee84e141922625e1d7c5b389b99bb3c572e0243607d5bb4d2'
assert hashlib.file_digest(base.open('rb'),'sha256').hexdigest()==expected_base
base_info=json.loads(subprocess.check_output(['qemu-img','info','--output=json',str(base)],text=True));assert base_info['format']=='qcow2'
out=e/distro;out.mkdir(exist_ok=False)
build=json.loads((r/'dist/packages/LINUX_PACKAGES_ACCEPTANCE.json' if args.published else e/'BUILD_RESULT.json').read_text())
if not args.published:assert build['status'].startswith('four-format build pass')
stage=r/'dist/packages' if args.published else Path(build['stage']);package=next(stage.glob('*.rpm' if distro=='fedora' else '*.deb'));portable=next(stage.glob('*portable.tar.gz'))
name='ottrin-native-'+distro+'-desktop-qa';mac='52:54:00:6f:74:'+('47' if distro=='fedora' else '48');prefix=name+'-';owned_volumes=[];domain_uuid=None
assert not args.gui_qa or distro=='ubuntu'
actor=args.actor.resolve() if args.actor else drivers/'native-actor.py'
assert actor.is_relative_to(r/'.hermes/evidence') and actor.is_file()
for artifact in [package,portable]:
 with artifact.open('rb') as f:assert hashlib.file_digest(f,'sha256').hexdigest()==build['artifacts'][artifact.name]
report={'setup_control':False,'distro':distro,'domain':name,'mac':mac,'source':build['source_revision'],'status':'running','package_sha256':hashlib.file_digest(package.open('rb'),'sha256').hexdigest()}
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
  for line in virsh('vol-list','default').stdout.splitlines()[2:]:
   fields=line.split()
   if fields:
    other=ET.fromstring(virsh('vol-dumpxml',fields[0],'--pool','default').stdout)
    assert path not in {x.text for x in other.findall('.//backingStore/path')}, 'Another volume retains this backing file'
  virsh('vol-delete',volume,'--pool','default')
 cleanup['volumes_removed']=owned_volumes.copy();(out/'VM_CLEANUP.json').write_text(json.dumps(cleanup,indent=2)+'\n')
try:
 with tempfile.TemporaryDirectory(prefix='.native-vm-',dir=out) as temporary,ExitStack() as cleanup:
  temporary=Path(temporary);cleanup.callback(teardown)
  key=temporary/'id_ed25519';run(['ssh-keygen','-q','-t','ed25519','-N','','-f',key])
  public=key.with_suffix('.pub').read_text().strip()
  (temporary/'user-data').write_text('#cloud-config\nhostname: '+name+'\nssh_pwauth: false\nusers:\n  - name: ottrinqa\n    groups: '+('wheel' if distro=='fedora' else 'sudo')+'\n    sudo:\n      - ALL=(ALL) NOPASSWD:ALL\n    shell: /bin/bash\n    ssh_authorized_keys:\n      - '+public+'\n')
  (temporary/'meta-data').write_text('instance-id: '+name+'\nlocal-hostname: '+name+'\n')
  seed=temporary/'seed.iso';run(['genisoimage','-quiet','-output',seed,'-volid','cidata','-joliet','-rock',temporary/'user-data',temporary/'meta-data'])
  for suffix,capacity,fmt,backing in [('base.qcow2',base_info['virtual-size'],'qcow2',None),('work.qcow2',12884901888,'qcow2',prefix+'base.qcow2'),('seed.iso',seed.stat().st_size,'raw',None)]:
   volume=prefix+suffix;xml=ET.Element('volume');ET.SubElement(xml,'name').text=volume;ET.SubElement(xml,'capacity',unit='bytes').text=str(capacity);target=ET.SubElement(xml,'target');ET.SubElement(target,'format',type=fmt)
   if backing:
    store=ET.SubElement(xml,'backingStore');ET.SubElement(store,'path').text=virsh('vol-path',backing,'--pool','default').stdout.strip();ET.SubElement(store,'format',type='qcow2')
   source=temporary/(suffix+'.xml');source.write_text(ET.tostring(xml,encoding='unicode'))
   virsh('vol-create','default',source);owned_volumes.append(volume)
   if suffix!='work.qcow2':virsh('vol-upload',volume,base if suffix=='base.qcow2' else seed,'--pool','default',timeout=180)
  (out/'VM_OWNER.json').write_text(json.dumps({'domain':name,'mac':mac,'volumes':owned_volumes,'base_cache':str(base),'cleanup_owner':str(Path(__file__)),'actor_sha256':hashlib.file_digest(actor.open('rb'),'sha256').hexdigest(),'controller_sha256':hashlib.file_digest(Path(__file__).open('rb'),'sha256').hexdigest(),'probe_sha256':hashlib.file_digest((r/'.hermes/evidence/native-desktop-fixed-current/portal-probe.py').open('rb'),'sha256').hexdigest()},indent=2)+'\n')
  domain_uuid=str(uuid.uuid4())
  run(['virt-install','--connect','qemu:///system','--name',name,'--uuid',domain_uuid,'--memory','3072','--vcpus','2','--import','--disk','vol=default/'+prefix+'work.qcow2,bus=virtio','--disk','vol=default/'+prefix+'seed.iso,device=cdrom','--network','network=default,mac='+mac,'--graphics','spice,listen=127.0.0.1','--video','virtio','--osinfo','generic','--noautoconsole'],timeout=60)
  assert virsh('domuuid',name).stdout.strip()==domain_uuid;report['uuid']=domain_uuid
  deadline=time.monotonic()+240;address=None
  while time.monotonic()<deadline:
   # Reused QA MACs can retain several unexpired cloud-init DUID leases.
   # domifaddr's first grouped address may belong to an already removed guest.
   leases=virsh('net-dhcp-leases','default','--mac',mac).stdout
   matching=[line.split() for line in leases.splitlines() if mac in line and 'ipv4' in line]
   address=sorted(matching,key=lambda fields:fields[:2])[-1][4].split('/')[0] if matching else None
   if address:
    ssh=['ssh','-F','/dev/null','-o','ForwardX11=no','-i',str(key),'-o','IdentitiesOnly=yes','-o','BatchMode=yes','-o','StrictHostKeyChecking=yes','-o','UserKnownHostsFile='+str(temporary/'known_hosts'),'-o','ConnectTimeout=4','ottrinqa@'+address]
    if not (temporary/'known_hosts').exists():
     scan=run(['ssh-keyscan','-T','3',address],check=False,timeout=6)
     if scan.returncode==0:(temporary/'known_hosts').write_text(scan.stdout)
    if (temporary/'known_hosts').exists() and run(ssh+['hostname'],check=False,timeout=8).stdout.strip()==name:break
   time.sleep(2)
  else:raise RuntimeError('Owned guest SSH unavailable')
  def remote(*args,check=True,timeout=180):return run(ssh+[shlex.join(list(map(str,args)))],check=check,timeout=timeout)
  cloud=remote('sudo','cloud-init','status','--wait','--format=json',check=False,timeout=180)
  (out/'VM_CLOUD_INIT.json').write_text(cloud.stdout)
  (out/'VM_CLOUD_INIT.stderr').write_text(cloud.stderr)
  cloud_state=json.loads(cloud.stdout)
  assert cloud.returncode in (0,2) and cloud_state['status']=='done' and not cloud_state.get('errors'),cloud_state
  report['cloud_init']={'exit':cloud.returncode,'status':cloud_state['status'],'recoverable_errors':cloud_state.get('recoverable_errors'),'scope':'Provisioning warnings retained separately; product gates unchanged'}
  scp=['scp','-F','/dev/null','-o','ForwardX11=no','-i',str(key),'-o','IdentitiesOnly=yes','-o','BatchMode=yes','-o','StrictHostKeyChecking=yes','-o','UserKnownHostsFile='+str(temporary/'known_hosts')]
  for source,destination in [(package,'candidate.rpm' if distro=='fedora' else 'candidate.deb'),(portable,'portable.tar.gz'),(actor,'actor.py'),(r/'.hermes/evidence/native-desktop-fixed-current/portal-probe.py','portal-probe.py')]:run(scp+[str(source),'ottrinqa@'+address+':'+destination],timeout=180)
  for extra in args.input:
   extra=extra.resolve();assert extra.is_relative_to(r) and extra.is_file()
   run(scp+[str(extra),'ottrinqa@'+address+':'+extra.name],timeout=60)
  if distro=='fedora':
   install=remote('sudo','dnf','-y','install','/home/ottrinqa/candidate.rpm','xdg-desktop-portal','xdg-desktop-portal-gtk','xorg-x11-server-Xvfb','xdotool','python3-gobject',check=False,timeout=600)
  else:
   update=remote('sudo','apt-get','update',check=False,timeout=300);(out/'VM_APT_UPDATE.log').write_text(update.stdout+update.stderr);assert update.returncode==0
   install=remote('sudo','env','DEBIAN_FRONTEND=noninteractive','apt-get','-y','--no-install-recommends','install','/home/ottrinqa/candidate.deb','xdg-desktop-portal','xdg-desktop-portal-gtk','xvfb','xdotool','python3-gi',*(['openbox','psmisc','policykit-1-gnome','python3-pyatspi','python3-pil','lightdm','lightdm-gtk-greeter','xserver-xorg','dbus-x11','dbus-user-session'] if args.gui_qa else []),check=False,timeout=600)
  (out/'VM_INSTALL.log').write_text(install.stdout+install.stderr)
  assert install.returncode==0,'Native package install failed; see VM_INSTALL.log'
  result=remote('sudo','python3','/home/ottrinqa/actor.py',build['source_revision'],distro,check=False,timeout=600);(out/'VM_ACTOR.log').write_text(result.stdout+result.stderr)
  run(scp+['ottrinqa@'+address+':results.tar.gz',str(out/'VM_RESULTS.tar.gz')],timeout=60)
  assert result.returncode==0,result.stdout[-2000:]+result.stderr[-1000:]
  report['status']='pass within actor scope'
except BaseException as error:
 report.update(status='fail',error=repr(error));raise
finally:
 report['temporary_seed_and_ssh_keys_removed']=not any(out.glob('.native-vm-*'));(out/'VM_RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
