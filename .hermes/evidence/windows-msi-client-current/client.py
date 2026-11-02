"""Bounded clean-client control with teardown registered before VM allocation."""
from pathlib import Path
from contextlib import ExitStack
import base64, hashlib, importlib.util, json, os, shutil, subprocess, time, urllib.request, uuid, xml.etree.ElementTree as E
r=Path.cwd();e=Path(__file__).resolve().parent;name='ottrin-msi-clean-client';uid=str(uuid.uuid4());volume=name+'.qcow2';iso_volume=name+'-fixture.iso'
def virsh(*args):return subprocess.check_output(['virsh','-c','qemu:///system',*args],text=True).strip()
def save(n,data):(e/n).write_text(json.dumps(data,indent=2)+'\n')
assert name not in virsh('list','--all','--name').splitlines()
assert virsh('domstate','win11ltsc')=='shut off'
assert virsh('domuuid','win11ltsc')=='4e160378-2610-4072-b528-048e4c9f509e'
assert not virsh('snapshot-list','win11ltsc','--name')
for v in [volume,iso_volume]:assert subprocess.run(['virsh','-c','qemu:///system','vol-info','--pool','default',v],capture_output=True).returncode!=0
pins=json.loads((r/'.hermes/evidence/windows-20261011/windows-msi-compiler-downloads.json').read_text())
cache=r/'.hermes/tools/msi-compiler';cache.mkdir(exist_ok=True)
for filename,url,algorithm,digest in [('runtime.zip',pins['runtime_url'],'sha512',pins['runtime_sha512']),('wix.zip',pins['wix_url'],'sha256',pins['wix_sha256'])]:
 p=cache/filename
 if not p.exists():
  temp=p.with_suffix('.download');assert not temp.exists()
  try:urllib.request.urlretrieve(url,temp);assert hashlib.new(algorithm,temp.read_bytes()).hexdigest()==digest;p.parent.mkdir(exist_ok=True);temp.rename(p)
  finally:temp.unlink(missing_ok=True)
 assert hashlib.new(algorithm,p.read_bytes()).hexdigest()==digest
fixture=e/'fixture';assert not fixture.exists();fixture.mkdir()
state={'domain':name,'uuid':uid,'base_domain':'win11ltsc','base_disk':'/var/lib/libvirt/images/win11ltsc.qcow2','network_interfaces':0,'volumes':[],'status':'preparing','public_compiler_cache':str(cache),'scope':'Fresh disposable offline client overlay; ordinary-user file-only MSI control. No original VM or user profile mutation.'}
save('OWNER.json',state)
def cleanup():
 try:
  if name in virsh('list','--all','--name').splitlines():
   assert virsh('domuuid',name)==uid
   assert not virsh('snapshot-list',name,'--name')
   root=E.fromstring(virsh('dumpxml',name));assert not root.findall('./devices/interface')
   disk_sources={x.get('file') for x in root.findall('./devices/disk/source')};assert disk_sources=={'/var/lib/libvirt/images/'+volume,'/var/lib/libvirt/images/'+iso_volume}
   if virsh('domstate',name)!='shut off':virsh('destroy',name)
   virsh('undefine',name,'--nvram','--tpm')
  for v in reversed(state['volumes']):
   for domain in virsh('list','--all','--name').splitlines():
    assert '/var/lib/libvirt/images/'+v not in virsh('dumpxml',domain)
   virsh('vol-delete','--pool','default',v)
  if fixture.exists():shutil.rmtree(fixture)
  state['cleanup']={'domain_removed':True,'owned_volumes_removed':state['volumes'][:],'fixture_removed':True,'base_still_off':virsh('domstate','win11ltsc')=='shut off'}
 except BaseException as error:state['cleanup_error']=repr(error);raise
 finally:save('OWNER.json',state)
with ExitStack() as stack:
 stack.callback(cleanup)
 for filename in ['runtime.zip','wix.zip']:os.link(cache/filename,fixture/filename)
 (fixture/'pins.json').write_text(json.dumps(pins))
 source=r/'.hermes/evidence/msi-hosted-control-current/source'
 actor=(source/'msi-rollback-control-actor.ps1').read_text()
 actor=actor.replace(" $profilePath=[Environment]", " $session=(Get-Process -Id $PID).SessionId;Require ($session-gt0) 'Real ordinary interactive session required';$result.session_id=$session\n $profilePath=[Environment]")
 (fixture/'actor.ps1').write_text(actor)
 script=(source/'msi-rollback-control.ps1').read_text()
 start=script.index("if($env:GITHUB_ACTIONS")
 end=script.index("$id=[Security.Principal.WindowsIdentity]",start)
 script=script[:start]+script[end:]
 script=script.replace("$env:RUNNER_TEMP","$env:TEMP")
 start=script.index(" $repo=[IO.Path]::GetFullPath")
 end=script.index(" Expand-Archive",start)
 script=script[:start]+" $pins=Get-Content (Join-Path $PSScriptRoot 'pins.json') -Raw|ConvertFrom-Json\n foreach($file in @('runtime.zip','wix.zip')){Copy-Item (Join-Path $PSScriptRoot $file) (Join-Path $root $file)}\n if((Get-FileHash (Join-Path $root 'runtime.zip') -Algorithm SHA512).Hash-ne$pins.runtime_sha512-or(Get-FileHash (Join-Path $root 'wix.zip')).Hash-ne$pins.wix_sha256){throw 'Offline compiler input mismatch'}\n"+script[end:]
 script=script.replace("(Join-Path $PSScriptRoot 'msi-rollback-control-actor.ps1')","(Join-Path $PSScriptRoot 'actor.ps1')")
 start=script.index(" $arguments='-NoProfile -NonInteractive -ExecutionPolicy")
 end=script.index(" $process=Start-Process",start)
 script=script[:start]+" $invoke=\"& ([ScriptBlock]::Create([IO.File]::ReadAllText('$(Join-Path $root 'actor.ps1')'))) -Root '$root' -ExpectedSid '$($user.SID.Value)'\"\n $arguments='-NoProfile -NonInteractive -EncodedCommand '+[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($invoke))\n"+script[end:]
 # A disposable overlay is the cleanup boundary for any engine registration defect.
 script=script.replace('hosted_runner_disposal_handles_any_residual_engine_registration','owned_overlay_disposal_handles_any_residual_engine_registration')
 # LocalSystem cannot use CreateProcessWithLogonW. Log on the fresh ordinary
 # account on this offline disposable clone instead; never alter the base.
 start=script.index(" $process=Start-Process")
 script=script[:start]+''' $action=New-ScheduledTaskAction -Execute "$env:SystemRoot\\System32\\WindowsPowerShell\\v1.0\\powershell.exe" -Argument $arguments
 $trigger=New-ScheduledTaskTrigger -AtLogOn -User ($env:COMPUTERNAME+'\\'+$name)
 $principal=New-ScheduledTaskPrincipal -UserId ($env:COMPUTERNAME+'\\'+$name) -LogonType Interactive -RunLevel Limited
 $taskName='OttrinOwnedMsiControl';Register-ScheduledTask -TaskName $taskName -Action $action -Trigger $trigger -Principal $principal -Force|Out-Null
 $plain=$credential.GetNetworkCredential().Password
 $key='HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Winlogon'
 foreach($pair in @(@('AutoAdminLogon','1'),@('DefaultUserName',$name),@('DefaultDomainName',$env:COMPUTERNAME),@('DefaultPassword',$plain))){Set-ItemProperty -Path $key -Name $pair[0] -Value $pair[1]}
 $plain=$null;$state.phase='ready for owned interactive reboot';$state.status='prepared';$state.task=$taskName;$state.actor_sid=$user.SID.Value;$exit=0;Save
}catch{$state.status='failed interactive preparation';$state.error=$_.Exception.Message;$state.stack=$_.ScriptStackTrace;$exit=1}
finally{if($password){$password.Dispose()};Save}
exit $exit
'''
 (fixture/'controller.ps1').write_text(script);shutil.copy2(fixture/'controller.ps1',e/'controller.ps1');shutil.copy2(fixture/'actor.ps1',e/'actor.ps1')
 subprocess.run(['genisoimage','-quiet','-J','-r','-V','OTMSICONTROL','-o',str(fixture/'inputs.iso'),str(fixture/'runtime.zip'),str(fixture/'wix.zip'),str(fixture/'pins.json'),str(fixture/'actor.ps1'),str(fixture/'controller.ps1')],check=True)
 virsh('vol-create-as','default',iso_volume,str((fixture/'inputs.iso').stat().st_size),'--format','raw');state['volumes'].append(iso_volume);save('OWNER.json',state)
 virsh('vol-upload','--pool','default',iso_volume,str(fixture/'inputs.iso'))
 virsh('vol-create-as','default',volume,'64G','--format','qcow2','--backing-vol',state['base_disk'],'--backing-vol-format','qcow2');state['volumes'].append(volume);save('OWNER.json',state)
 root=E.fromstring(virsh('dumpxml','win11ltsc'));root.find('name').text=name;root.find('uuid').text=uid
 if root.find('genid') is not None:root.find('genid').text=str(uuid.uuid4())
 nv=root.find('./os/nvram');nv.text='/var/lib/libvirt/qemu/nvram/'+name+'_VARS.fd'
 disks=root.findall('./devices/disk');assert len(disks)==1;disks[0].find('source').set('file','/var/lib/libvirt/images/'+volume)
 for interface in root.findall('./devices/interface'):root.find('devices').remove(interface)
 cd=E.SubElement(root.find('devices'),'disk',{'type':'file','device':'cdrom'});E.SubElement(cd,'driver',{'name':'qemu','type':'raw'});E.SubElement(cd,'source',{'file':'/var/lib/libvirt/images/'+iso_volume});E.SubElement(cd,'target',{'dev':'sda','bus':'sata'});E.SubElement(cd,'readonly')
 xml=fixture/'domain.xml';E.ElementTree(root).write(xml,encoding='unicode');virsh('define',str(xml));virsh('start',name);state['status']='booting';save('OWNER.json',state)
 spec=importlib.util.spec_from_file_location('guest',r/'.hermes/evidence/windows-20261011/guest_exec.py');g=importlib.util.module_from_spec(spec);spec.loader.exec_module(g);g.DOMAIN=name
 deadline=time.monotonic()+180
 while True:
  try:g.qga('guest-ping');break
  except RuntimeError:
   if time.monotonic()>deadline:raise
   time.sleep(2)
 def ps(code,timeout=500):
  encoded=base64.b64encode(code.encode('utf-16le')).decode();pid=g.qga('guest-exec',{'path':r'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe','arg':['-NoProfile','-NonInteractive','-EncodedCommand',encoded],'capture-output':True})['pid'];end=time.monotonic()+timeout
  state['guest_pid']=pid;save('OWNER.json',state)
  while time.monotonic()<end:
   result=g.qga('guest-exec-status',{'pid':pid})
   if result.get('exited'):return result
   time.sleep(1)
  raise TimeoutError('Owned guest control timed out; overlay teardown follows')
 result=ps("$ErrorActionPreference='Stop';$o=Get-CimInstance Win32_OperatingSystem;@{caption=$o.Caption;build=$o.BuildNumber;msi=(Get-Item \"$env:SystemRoot\\System32\\msi.dll\").VersionInfo.FileVersion;disable_msi=(Get-ItemProperty 'HKLM:\\Software\\Policies\\Microsoft\\Windows\\Installer' -ErrorAction SilentlyContinue).DisableMsi}|ConvertTo-Json -Compress")
 assert result['exitcode']==0;save('CLIENT_VERSION.json',json.loads(base64.b64decode(result['out-data']).decode('utf-8-sig')))
 state['status']='running ordinary-user native control';save('OWNER.json',state)
 code="$ErrorActionPreference='Stop';$disc=@(Get-CimInstance Win32_LogicalDisk|Where-Object {$_.VolumeName-eq'OTMSICONTROL'});if($disc.Count-ne1){throw 'Owned fixture disc missing'};$script=Join-Path ($disc[0].DeviceID+'\\') 'controller.ps1';$out='C:\\OttrinMsiCleanClientResults';$source=[IO.File]::ReadAllText($script);$source=$source.Replace('$PSScriptRoot',\"'\"+(Split-Path $script)+\"'\"); & ([ScriptBlock]::Create($source)) -Output $out"
 result=ps(code);save('CONTROLLER_PROCESS.json',{'exitcode':result.get('exitcode'),'stdout':base64.b64decode(result.get('out-data','')).decode('utf-8',errors='replace'),'stderr':base64.b64decode(result.get('err-data','')).decode('utf-8',errors='replace')})
 assert result['exitcode']==0,'Interactive preparation failed; preserve controller evidence'
 prepared=ps("Get-Content 'C:\\OttrinMsiCleanClientResults\\OWNER.json' -Raw",30)
 owner=json.loads(base64.b64decode(prepared['out-data']).decode('utf-8-sig'));assert owner['status']=='prepared';save('GUEST_PREPARATION.json',owner)
 ps("shutdown.exe /r /t 3 /f",15);state['status']='waiting for owned interactive actor after reboot';save('OWNER.json',state)
 time.sleep(8);deadline=time.monotonic()+400
 while True:
  try:
   result=ps("$p='"+owner['root']+"\\results\\RESULT.json';if(Test-Path $p){Get-Content $p -Raw}else{'{}'}",20)
   if result.get('exitcode')==0:
    actor_result=json.loads(base64.b64decode(result.get('out-data','e30=')).decode('utf-8-sig'))
    if actor_result.get('status') not in [None,'running']:break
  except (RuntimeError,TimeoutError,json.JSONDecodeError):pass
  assert time.monotonic()<deadline,'Interactive actor did not finish'
  time.sleep(3)
 save('GUEST_RESULT.json',actor_result)
 result=ps("$ErrorActionPreference='Stop';$owner=Get-Content 'C:\\OttrinMsiCleanClientResults\\OWNER.json' -Raw|ConvertFrom-Json;Copy-Item ($owner.root+'\\results\\*') 'C:\\OttrinMsiCleanClientResults';Compress-Archive 'C:\\OttrinMsiCleanClientResults\\*' 'C:\\OttrinMsiCleanClientResults.zip';[Convert]::ToBase64String([IO.File]::ReadAllBytes('C:\\OttrinMsiCleanClientResults.zip'))",30)
 assert result['exitcode']==0
 (e/'RESULTS.zip').write_bytes(base64.b64decode(base64.b64decode(result['out-data']).decode().strip()))
 state['status']='control finished; inspect exact result, not assumed pass';save('OWNER.json',state)
print('Clean client control and owned overlay teardown finished.',flush=True)
