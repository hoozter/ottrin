"""Bounded clean-client control with teardown registered before VM allocation."""
from pathlib import Path
from contextlib import ExitStack
import argparse, base64, hashlib, importlib.util, json, os, shutil, signal, subprocess, time, urllib.request, uuid, xml.etree.ElementTree as E
def cancelled(signum, frame):raise SystemExit(128+signum)
for sig in [signal.SIGTERM,signal.SIGINT]:signal.signal(sig,cancelled)
parser=argparse.ArgumentParser();parser.add_argument('--update-manifest',type=Path);parser.add_argument('--output',type=Path);parser.add_argument('--msix-parse-only',action='store_true');args=parser.parse_args()
assert not(args.update_manifest and args.msix_parse_only)
assert bool(args.output)==bool(args.update_manifest or args.msix_parse_only),'New comparison needs an explicit new evidence root'
r=Path.cwd();e=args.output.resolve() if args.output else Path(__file__).resolve().parent
assert not args.output or not (e/'OWNER.json').exists(),'Do not replace an existing owned run'
name='ottrin-msix-parser' if args.msix_parse_only else 'ottrin-msi-updated-client' if args.update_manifest else 'ottrin-msi-clean-client';uid=str(uuid.uuid4());volume=name+'.qcow2';iso_volume=name+'-fixture.iso'
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
fixture=e/'fixture';assert not fixture.exists()
scope='Native MSIX parser/XML and SDK smoke; assembly/deployment remain separate' if args.msix_parse_only else 'Fresh disposable offline client overlay; ordinary-user file-only MSI control. No original VM or user profile mutation.'
state={'domain':name,'uuid':uid,'base_domain':'win11ltsc','base_disk':'/var/lib/libvirt/images/win11ltsc.qcow2','network_interfaces':0,'volumes':[],'status':'preparing','public_compiler_cache':str(cache),'scope':scope,'producer_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
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
 fixture.mkdir()
 for filename in ['runtime.zip','wix.zip']:os.link(cache/filename,fixture/filename)
 if args.update_manifest:
  inputs=json.loads(args.update_manifest.read_text());update_cache=r/'.hermes/tools/windows-client-update'
  assert shutil.disk_usage('/var/lib/libvirt/images').free>24*1024**3,'Need room for owned ISO, servicing overlay and teardown'
  for item in inputs['files']:
   path=update_cache/item['filename']
   with path.open('rb') as f:assert hashlib.file_digest(f,'sha256').hexdigest()==item['sha256']
   os.link(path,fixture/path.name)
 if args.update_manifest or args.msix_parse_only:
  for filename in ['package-msix.ps1','msix-manifest.xml']:
   shutil.copy2(r/'packaging/windows'/filename,fixture/filename)
  os.link(cache/'windows-sdk-buildtools.nupkg',fixture/'windows-sdk-buildtools.zip')
  shutil.copy2(r/'packaging/tests/windows-msix-template.ps1',fixture/'windows-msix-template.ps1')
  shutil.copy2(r/'assets/icon.png',fixture/'icon.png')
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
 iso_inputs=[fixture/n for n in ['runtime.zip','wix.zip','pins.json','actor.ps1','controller.ps1']]
 if args.update_manifest:iso_inputs.extend(fixture/item['filename'] for item in inputs['files'])
 if args.update_manifest or args.msix_parse_only:
  iso_inputs.extend(fixture/name for name in ['package-msix.ps1','msix-manifest.xml','windows-sdk-buildtools.zip','windows-msix-template.ps1','icon.png'])
 subprocess.run(['genisoimage','-quiet','-udf','-iso-level','3','-J','-joliet-long','-r','-V','OTMSICONTROL','-o',str(fixture/'inputs.iso'),*map(str,iso_inputs)],check=True)
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
 if args.update_manifest or args.msix_parse_only:
  result=ps("$ErrorActionPreference='Stop';$disc=@(Get-CimInstance Win32_LogicalDisk|Where-Object {$_.VolumeName-eq'OTMSICONTROL'});if($disc.Count-ne1){throw 'Owned disc missing'};$script=Join-Path ($disc[0].DeviceID+'\\') 'package-msix.ps1';$errors=$null;$tokens=$null;$null=[Management.Automation.Language.Parser]::ParseFile($script,[ref]$tokens,[ref]$errors);if($errors.Count){throw ($errors|Out-String)};[xml]$manifest=Get-Content (Join-Path ($disc[0].DeviceID+'\\') 'msix-manifest.xml') -Raw;@{parser='Windows PowerShell 5.1';syntax_errors=0;source_sha256=(Get-FileHash $script).Hash;manifest_sha256=(Get-FileHash (Join-Path ($disc[0].DeviceID+'\\') 'msix-manifest.xml')).Hash;scope='Native parser and XML load; not package assembly/deployment'}|ConvertTo-Json -Compress",30)
  save('MSIX_NATIVE_PARSE.json',{'process_exit':result.get('exitcode'),'stdout':base64.b64decode(result.get('out-data','')).decode('utf-8-sig'),'stderr':base64.b64decode(result.get('err-data','')).decode('utf-8',errors='replace')});assert result['exitcode']==0
  code="$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$disc=@(Get-CimInstance Win32_LogicalDisk|Where-Object {$_.VolumeName-eq'OTMSICONTROL'});$d=$disc[0].DeviceID+'\\';$root='C:\\OttrinOwnedMsixSdk';if(Test-Path $root){throw 'Existing SDK fixture retained'};New-Item -ItemType Directory $root|Out-Null;Expand-Archive (Join-Path $d 'windows-sdk-buildtools.zip') $root;$tool=Join-Path $root 'bin\\10.0.26100.0\\x64\\makeappx.exe'; & ([ScriptBlock]::Create([IO.File]::ReadAllText((Join-Path $d 'windows-msix-template.ps1')))) -MakeAppx $tool -Template (Join-Path $d 'msix-manifest.xml') -Icon (Join-Path $d 'icon.png')"
  result=ps(code,90);save('SDK_NATIVE_SMOKE.json',{'process_exit':result.get('exitcode'),'stdout':base64.b64decode(result.get('out-data','')).decode('utf-8-sig'),'stderr':base64.b64decode(result.get('err-data','')).decode('utf-8',errors='replace')});assert result['exitcode']==0
  if args.msix_parse_only:
   state['status']='native parser/XML and SDK invocation pass; assembly/deployment remain required';save('OWNER.json',state)
   raise SystemExit(0)
 if args.update_manifest:
  state['status']='servicing owned offline clone with supported Microsoft update';save('OWNER.json',state)
  target=next(item for item in inputs['files'] if 'kb5127648' in item['filename'])
  code="$ErrorActionPreference='Stop';$disc=@(Get-CimInstance Win32_LogicalDisk|Where-Object {$_.VolumeName-eq'OTMSICONTROL'});if($disc.Count-ne1){throw 'Owned update disc missing'};$msu=Join-Path ($disc[0].DeviceID+'\\') '"+target['filename']+"';$p=Start-Process wusa.exe -ArgumentList ('\"'+$msu+'\" /quiet /norestart') -PassThru;$null=$p.Handle;if(-not$p.WaitForExit(1500000)){$p.Kill();throw 'Owned update timed out'};$p.WaitForExit();$code=$p.ExitCode;@{exitcode=$code;update='KB5127648';scope='Only disposable network-isolated clone; WUSA validates update applicability and trust'}|ConvertTo-Json -Compress;if($code-notin@(0,3010,2359302)){exit 1}"
  result=ps(code,1600);text=base64.b64decode(result.get('out-data','')).decode('utf-8-sig');save('SERVICING.json',{'process_exit':result.get('exitcode'),'output':text,'stderr':base64.b64decode(result.get('err-data','')).decode('utf-8',errors='replace')});assert result['exitcode']==0,'Supported update failed; no policy or signature bypass'
  ps('shutdown.exe /r /t 3 /f',15);time.sleep(8);deadline=time.monotonic()+600
  while True:
   try:
    result=ps("$v=Get-ItemProperty 'HKLM:\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion';@{build=$v.CurrentBuildNumber;ubr=$v.UBR;msi=(Get-Item \"$env:SystemRoot\\System32\\msi.dll\").VersionInfo.FileVersion}|ConvertTo-Json -Compress",20)
    if result.get('exitcode')==0:
     version=json.loads(base64.b64decode(result['out-data']).decode('utf-8-sig'))
     if version['build']=='26100' and version['ubr']==9711:break
   except (RuntimeError,TimeoutError,json.JSONDecodeError):pass
   assert time.monotonic()<deadline,'Updated clone did not reach exact target build'
   time.sleep(3)
  save('UPDATED_CLIENT_VERSION.json',version)
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
