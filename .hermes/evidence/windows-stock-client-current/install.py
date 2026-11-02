"""Install one independent Microsoft evaluation client with registered owned teardown."""
from pathlib import Path
from contextlib import ExitStack
import base64,hashlib,importlib.util,json,os,secrets,shutil,signal,subprocess,time,uuid,xml.etree.ElementTree as ET
r=Path.cwd();e=Path(__file__).resolve().parent;cache=r/'.hermes/tools/windows-stock-client';fixture=r/'.hermes/packaging-tmp/.windows-stock-client-fixture';name='ottrin-stock-client';uid=str(uuid.uuid4());nonce=uuid.uuid4().hex;disk=name+'.qcow2';media=name+'-windows.iso';seed=name+'-seed.iso';volumes=[];retained=None;defined=False
assert not fixture.exists()
def run(args,check=True,timeout=600):
 result=subprocess.run(args,text=True,capture_output=True,timeout=timeout)
 if check and result.returncode:
  (e/'NATIVE_COMMAND_FAILURE.json').write_text(json.dumps({'command':list(map(str,args)),'exit':result.returncode,'stdout':result.stdout,'stderr':result.stderr},indent=2)+'\n')
  raise RuntimeError(result.stderr or result.stdout)
 return result
def virsh(*args):return run(['virsh','-c','qemu:///system',*map(str,args)]).stdout.strip()
assert name not in virsh('list','--all','--name').splitlines()
assert not any(v in virsh('vol-list','default') for v in [disk,seed])
adopt_media=media in virsh('vol-list','default').split()
if adopt_media:
 pending=json.loads((e/'PENDING_MEDIA.json').read_text());assert pending['volume']==media and pending['upload_completed']
 for other in virsh('list','--all','--name').splitlines():assert '/var/lib/libvirt/images/'+media not in virsh('dumpxml',other)
 volumes.append(media)
verification=json.loads((e/'WINDOWS_SOURCE_VERIFICATION.json').read_text());assert verification['status'].startswith('pass');source=Path(verification['path']);assert adopt_media or source.stat().st_size==verification['bytes'];bootstrap=json.loads((e/'BOOTSTRAP_OWNER.json').read_text());assert bootstrap['status'].startswith('extracted')
state={'domain':name,'uuid':uid,'nonce':nonce,'status':'preparing independent installation','volumes':volumes,'source':verification,'cleanup_owner':str(Path(__file__).resolve()),'scope':'Clean Microsoft Windows installation and bootstrap only. No MSI rollback or Ottrin acceptance inferred.','host_filesystems':0,'host_devices':0,'usb_redirection':0,'network_interfaces':0}
def save():
 p=e/'OWNER.json.tmp';p.write_text(json.dumps(state,indent=2)+'\n');p.replace(e/'OWNER.json')
def cancelled(*_):raise KeyboardInterrupt('Owned independent client installation cancelled')
for sig in [signal.SIGTERM,signal.SIGINT]:signal.signal(sig,cancelled)
if adopt_media:
 assert pending['source']['sha256']==verification['sha256'];state['media_upload_complete']=True
save()
def cleanup():
 global retained
 if state.get('installation_observed') and retained is None:
  retained=disk;state['recovery_required']='Windows installation observed by live agent; preserve sole disk for exact bootstrap/observer recovery, not accepted and must not repeat installation'
  (e/'RECOVERY.json').write_text(json.dumps({'status':'pending observed-installation recovery; not acceptance','disk':'/var/lib/libvirt/images/'+disk,'source':verification,'nonce':nonce,'cleanup_owner':str(Path(__file__).resolve()),'next':'Resume same installed disk/template, inspect exact BOOTSTRAP.json and finish native trust verification. Do not reinstall from ISO.'},indent=2)+'\n')
 try:
  if defined and name in virsh('list','--all','--name').splitlines():
   assert virsh('domuuid',name)==uid;assert not virsh('snapshot-list',name,'--name')
   if virsh('domstate',name)!='shut off':virsh('destroy',name)
   virsh('undefine',name,'--nvram','--tpm')
  keep_media=state.get('media_upload_complete') and not state.get('installation_observed')
  if keep_media:
   (e/'PENDING_MEDIA.json').write_text(json.dumps({'volume':media,'upload_completed':True,'source':verification,'purpose':'Verified Microsoft input retained for its unfulfilled OS installation consumer; adopt this sole volume instead of downloading/uploading again.','cleanup_owner':str(Path(__file__).resolve())},indent=2)+'\n')
  else:(e/'PENDING_MEDIA.json').unlink(missing_ok=True)
  removed=[]
  for v in reversed(volumes):
   if v==retained or (v==media and keep_media):continue
   for other in virsh('list','--all','--name').splitlines():assert '/var/lib/libvirt/images/'+v not in virsh('dumpxml',other)
   if v in virsh('vol-list','default').split():virsh('vol-delete','--pool','default',v);removed.append(v)
  if fixture.exists():shutil.rmtree(fixture)
  state['cleanup']={'domain_removed':not defined or name not in virsh('list','--all','--name').splitlines(),'owned_volumes_removed':removed,'fixture_removed':not fixture.exists(),'retained_pending_volume':retained,'retained_pending_input':media if keep_media else None}
 except BaseException as error:state['cleanup_error']=repr(error);raise
 finally:save()
sp=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');desktop=importlib.util.module_from_spec(sp);sp.loader.exec_module(desktop)
def make_domain():
 domain=ET.fromstring(virsh('dumpxml','win11ltsc'));domain.find('name').text=name;domain.find('uuid').text=uid
 for key in ['memory','currentMemory']:domain.find(key).text='4194304'
 domain.find('vcpu').text='4';domain.find('./os/loader').text='/usr/share/OVMF/OVMF_CODE_4M.ms.fd';nv=domain.find('./os/nvram');nv.text='/var/lib/libvirt/qemu/nvram/'+name+'_VARS.fd';nv.set('template','/usr/share/OVMF/OVMF_VARS_4M.ms.fd')
 for feature in domain.findall('./os/firmware/feature'):feature.set('enabled','yes')
 devices=domain.find('devices')
 for tag in ['disk','interface','filesystem','hostdev','redirdev','channel','tpm']:
  for node in devices.findall(tag):devices.remove(node)
 for node in domain.findall('./os/boot'):domain.find('os').remove(node)
 for path,dev,kind,order in [('/var/lib/libvirt/images/'+disk,'sda','disk',2),('/var/lib/libvirt/images/'+media,'sdb','cdrom',1),('/var/lib/libvirt/images/'+seed,'sdc','cdrom',None)]:
  node=ET.SubElement(devices,'disk',{'type':'file','device':kind});ET.SubElement(node,'driver',{'name':'qemu','type':'qcow2' if kind=='disk' else 'raw'});ET.SubElement(node,'source',{'file':path});ET.SubElement(node,'target',{'dev':dev,'bus':'sata'})
  if kind=='cdrom':ET.SubElement(node,'readonly')
  if order:ET.SubElement(node,'boot',{'order':str(order)})
 if not devices.findall("input[@type='tablet']"):ET.SubElement(devices,'input',{'type':'tablet','bus':'usb'})
 tpm=ET.SubElement(devices,'tpm',{'model':'tpm-crb'});ET.SubElement(tpm,'backend',{'type':'emulator','version':'2.0'})
 channel=ET.SubElement(devices,'channel',{'type':'unix'});ET.SubElement(channel,'source',{'mode':'bind'});ET.SubElement(channel,'target',{'type':'virtio','name':'org.qemu.guest_agent.0'})
 graphics=devices.find('graphics')
 for node in list(graphics.findall('gl')):graphics.remove(node)
 acceleration=devices.find('./video/model/acceleration')
 if acceleration is not None:acceleration.set('accel3d','no')
 return domain
with ExitStack() as stack:
 stack.callback(cleanup);fixture.mkdir(mode=0o700)
 domain=make_domain();xml=fixture/'domain.xml';ET.ElementTree(domain).write(xml,encoding='unicode');shutil.copy2(xml,e/'DEFINED_DOMAIN_INPUT.xml');virsh('define',xml);defined=True;save()
 if not adopt_media:
  assert shutil.disk_usage(r).free>source.stat().st_size+4*1024**3,'Need transfer margin before duplicating ISO into libvirt'
  with source.open('rb') as f:assert hashlib.file_digest(f,'sha256').hexdigest()==verification['sha256']
  virsh('vol-create-as','default',media,str(source.stat().st_size),'--format','raw');volumes.append(media);save();virsh('vol-upload','--pool','default',media,source)
  state['media_upload_complete']=True;save()
  assert source.stat().st_nlink==1;assert run(['fuser',str(source)],check=False).returncode==1;state['uploaded_media_host_copy_removed_bytes']=source.stat().st_size;source.unlink();save()
 for item in bootstrap['payloads']:
  p=cache/'bootstrap'/item['path'];assert hashlib.sha256(p.read_bytes()).hexdigest()==item['sha256'];to=fixture/item['path'];to.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,to)
 shutil.copy2(e/'bootstrap.ps1',fixture/'bootstrap.ps1');shutil.copy2(e/'bootstrap.ps1',e/'BOOTSTRAP_INPUT.ps1')
 (fixture/'bootstrap-input.json').write_text(json.dumps({'nonce':nonce,'files':bootstrap['payloads']}))
 # Supported Windows deployment settings; fresh owned disk0 only. No bypass keys,
 # modified installation image, third-party OS, activation key or security toggle.
 ns='urn:schemas-microsoft-com:unattend';wcm='http://schemas.microsoft.com/WMIConfig/2002/State';ET.register_namespace('',ns);ET.register_namespace('wcm',wcm)
 def add(parent,tag,text=None,**attrs):
  node=ET.SubElement(parent,'{'+ns+'}'+tag,attrs)
  if text is not None:node.text=str(text)
  return node
 def component(parent,name):return add(parent,'component',name=name,processorArchitecture='amd64',publicKeyToken='31bf3856ad364e35',language='neutral',versionScope='nonSxS')
 root=ET.Element('{'+ns+'}unattend');pe=add(root,'settings',pass_='windowsPE');pe.attrib={'pass':'windowsPE'};intl=component(pe,'Microsoft-Windows-International-Core-WinPE')
 for key,value in [('InputLocale','0409:00000409'),('SystemLocale','en-US'),('UILanguage','en-US'),('UserLocale','en-US')]:add(intl,key,value)
 add(add(intl,'SetupUILanguage'),'UILanguage','en-US');setup=component(pe,'Microsoft-Windows-Setup');user=add(setup,'UserData');add(user,'AcceptEula','true');add(user,'FullName','Ottrin QA');add(user,'Organization','Ottrin QA')
 dc=add(setup,'DiskConfiguration');d=add(dc,'Disk',**{'{'+wcm+'}action':'add'});add(d,'DiskID','0');add(d,'WillWipeDisk','true');creates=add(d,'CreatePartitions')
 for order,kind,size in [(1,'EFI',260),(2,'MSR',16),(3,'Primary',None)]:
  part=add(creates,'CreatePartition',**{'{'+wcm+'}action':'add'});add(part,'Order',order);add(part,'Type',kind);add(part,'Extend','true') if size is None else add(part,'Size',size)
 modifications=add(d,'ModifyPartitions')
 for order,pid,fmt,label in [(1,1,'FAT32','System'),(2,3,'NTFS','Windows')]:
  part=add(modifications,'ModifyPartition',**{'{'+wcm+'}action':'add'});add(part,'Order',order);add(part,'PartitionID',pid);add(part,'Format',fmt);add(part,'Label',label)
  if pid==3:add(part,'Letter','C')
 add(dc,'WillShowUI','OnError');osimage=add(add(setup,'ImageInstall'),'OSImage');meta=add(add(osimage,'InstallFrom'),'MetaData',**{'{'+wcm+'}action':'add'});add(meta,'Key','/IMAGE/INDEX');add(meta,'Value','1');dest=add(osimage,'InstallTo');add(dest,'DiskID','0');add(dest,'PartitionID','3');add(osimage,'WillShowUI','OnError')
 specsettings=add(root,'settings',**{'pass':'specialize'});shell=component(specsettings,'Microsoft-Windows-Shell-Setup');add(shell,'ComputerName','OTSTOCK'+nonce[:8]);add(shell,'TimeZone','UTC')
 oobe=add(root,'settings',**{'pass':'oobeSystem'});shell=component(oobe,'Microsoft-Windows-Shell-Setup');accounts=add(add(shell,'UserAccounts'),'LocalAccounts');account=add(accounts,'LocalAccount',**{'{'+wcm+'}action':'add'});password=secrets.token_hex(20)+'aA1!';pw=add(account,'Password');add(pw,'Value',password);add(pw,'PlainText','true');add(account,'Name','OttrinStockAdmin');add(account,'DisplayName','Ottrin Stock QA');add(account,'Group','Administrators')
 auto=add(shell,'AutoLogon');pw=add(auto,'Password');add(pw,'Value',password);add(pw,'PlainText','true');add(auto,'Username','OttrinStockAdmin');add(auto,'Enabled','true');add(auto,'LogonCount','1');password=None
 cmds=add(shell,'FirstLogonCommands');cmd=add(cmds,'SynchronousCommand',**{'{'+wcm+'}action':'add'});add(cmd,'Order','1');add(cmd,'Description','Install exact upstream QA serial/agent bootstrap')
 bootstrap_command=r'''$ErrorActionPreference='Stop';$d=@(Get-CimInstance Win32_LogicalDisk|Where-Object {$_.VolumeName-eq'OTSTOCKSEED'});if($d.Count-ne1){throw 'Owned bootstrap disc missing'};$media=$d[0].DeviceID+'\';& ([ScriptBlock]::Create([IO.File]::ReadAllText((Join-Path $media 'bootstrap.ps1')))) -Media $media'''
 add(cmd,'CommandLine','powershell.exe -NoProfile -NonInteractive -EncodedCommand '+base64.b64encode(bootstrap_command.encode('utf-16le')).decode());add(cmd,'RequiresUserInput','false');ET.ElementTree(root).write(fixture/'Autounattend.xml',encoding='utf-8',xml_declaration=True)
 seed_files=[fixture/item['path'] for item in bootstrap['payloads']]+[fixture/'bootstrap.ps1',fixture/'bootstrap-input.json',fixture/'Autounattend.xml']
 run(['genisoimage','-quiet','-udf','-iso-level','3','-J','-joliet-long','-r','-V','OTSTOCKSEED','-o',str(fixture/'seed.iso'),'-graft-points',*[str(p.relative_to(fixture))+'='+str(p) for p in seed_files]],timeout=120)
 virsh('vol-create-as','default',seed,str((fixture/'seed.iso').stat().st_size),'--format','raw');volumes.append(seed);save();virsh('vol-upload','--pool','default',seed,fixture/'seed.iso');(fixture/'seed.iso').unlink()
 virsh('vol-create-as','default',disk,'64G','--format','qcow2');volumes.append(disk);save()
 virsh('start',name);state['status']='installing clean Microsoft evaluation; no comparison acceptance yet';save()
 console=stack.enter_context(desktop.OwnedDesktop(e/'viewer'));viewer=console.spawn(['virt-viewer','--connect','qemu:///system','--attach','--domain-name','--reconnect','--auto-resize=never','--spice-disable-audio','--spice-disable-usbredir',name],'viewer.log');time.sleep(2);state['private_console']={'pid':viewer.pid,'display':console.env['DISPLAY'],'home':str(console.profile/'home')};save()
 # Recognize the installer boot prompt before delivering its single Return.
 until=time.monotonic()+25
 while time.monotonic()<until:
  found=desktop.run(['xdotool','search','--onlyvisible','--pid',str(viewer.pid)],console.env,False).stdout.split()
  if found:
   w=found[-1];desktop.run(['import','-window',w,str(e/'boot.png')],console.env);text=run(['tesseract',str(e/'boot.png'),'stdout','--psm','11'],check=False).stdout.casefold()
   if 'press any key' in text:
    desktop.run(['xdotool','windowactivate','--sync',w,'key','--clearmodifiers','Return'],console.env);state['boot_prompt_key']='One Return delivered after actual owned boot prompt OCR';save();break
  time.sleep(.3)
 sp=importlib.util.spec_from_file_location('guest',r/'.hermes/evidence/windows-20261011/guest_exec.py');guest=importlib.util.module_from_spec(sp);sp.loader.exec_module(guest);guest.DOMAIN=name
 while True:
  assert virsh('domstate',name) in ['running','blocked'],'Owned installer stopped before accepted bootstrap'
  assert shutil.disk_usage(r).free>3*1024**3,'Owned installation exhausted its storage margin'
  try:
   guest.qga('guest-ping');break
  except RuntimeError:time.sleep(5)
 state['installation_observed']=True;save();ET.ElementTree(ET.fromstring(virsh('dumpxml',name))).write(e/'DOMAIN_TEMPLATE.xml',encoding='unicode')
 while True:
  code="$p='C:\\OttrinStockClient\\BOOTSTRAP.json';$s=[IO.File]::Open($p,'Open','Read',([IO.FileShare]::ReadWrite-bor[IO.FileShare]::Delete));$reader=[IO.StreamReader]::new($s);try{$reader.ReadToEnd()}finally{$reader.Dispose()}"
  task=guest.qga('guest-exec',{'path':r'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe','arg':['-NoProfile','-NonInteractive','-Command',code],'capture-output':True});pid=task['pid']
  while True:
   value=guest.qga('guest-exec-status',{'pid':pid})
   if value.get('exited'):break
   time.sleep(1)
  if value.get('exitcode')==0:
   result=json.loads(base64.b64decode(value['out-data']).decode('utf-8-sig'));assert result['nonce']==nonce
   if result['status']=='failed bootstrap':raise RuntimeError(result)
   if result['status']=='ready for independent MSI comparison':break
  time.sleep(1)
 assert result['windows']['build']=='26300';(e/'BOOTSTRAP_RESULT.json').write_text(json.dumps(result,indent=2)+'\n')
 task=guest.qga('guest-exec',{'path':r'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe','arg':['-NoProfile','-NonInteractive','-Command',"$d=Get-BitLockerVolume -MountPoint 'C:';@{protection_status=[string]$d.ProtectionStatus;volume_status=[string]$d.VolumeStatus}|ConvertTo-Json -Compress"],'capture-output':True})
 while True:
  value=guest.qga('guest-exec-status',{'pid':task['pid']})
  if value.get('exited'):break
  time.sleep(1)
 assert value.get('exitcode')==0,value;encryption=json.loads(base64.b64decode(value['out-data']).decode('utf-8-sig'));(e/'ENCRYPTION_OBSERVATION.json').write_text(json.dumps(encryption,indent=2)+'\n');assert encryption['protection_status']=='Off','Do not clone a TPM-protected stock volume or disable its protection'
 virsh('shutdown',name,'--mode','agent');deadline=time.monotonic()+90
 while virsh('domstate',name)!='shut off':assert time.monotonic()<deadline;time.sleep(1)
 clean=ET.fromstring(virsh('dumpxml',name))
 for node in list(clean.findall('./devices/disk')):
  if node.get('device')=='cdrom':clean.find('devices').remove(node)
 clean.find('./devices/disk/boot').set('order','1');ET.ElementTree(clean).write(e/'DOMAIN_TEMPLATE.xml',encoding='unicode')
 retained=disk;state.update(status='clean independent client installed; native MSI comparison pending',retained_pending_volume=retained);save();(e/'ACCEPTANCE.json').write_text(json.dumps({'status':'pass within clean Microsoft install/bootstrap scope; native MSI control pending','disk':'/var/lib/libvirt/images/'+disk,'backing_image':None,'windows':result['windows'],'source':verification,'bootstrap_result':'BOOTSTRAP_RESULT.json','domain_template':'DOMAIN_TEMPLATE.xml','clean_agent_shutdown':True,'encryption_observation':encryption,'pending_consumers':['Independent native MSI executed-operation rollback comparison','Actual Ottrin MSI lifecycle only after minimal control is understood'],'cleanup_owner':str(Path(__file__).resolve())},indent=2)+'\n')
 print('Independent client installation/bootstrap complete; named MSI comparison consumer pending')
