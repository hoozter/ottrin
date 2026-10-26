"""Owned bounded interactive package session; teardown runs on every exit."""
from pathlib import Path
import sys, json, subprocess, time, hashlib, signal, shutil
r=Path.cwd();p=Path(__file__).resolve().parent;e=r/'.hermes/evidence/windows-copy-gui-current';e.mkdir(exist_ok=True)
sys.path.insert(0,str(p));import native_compiler as n
root=r'C:\OttrinWindowQA';domain=n.guest.DOMAIN
state={'status':'starting','guest_root':root,'domain':domain,'phase':'boot'};started=False;prepared=False
expected=json.loads((p/'HOST_PAYLOAD.json').read_text());archive=r/'.hermes/packaging-tmp/.windows-ci-stage/Ottrin-0.1.0-windows-x64-portable.zip'
nonce=__import__('uuid').uuid4().hex
state['nonce']=nonce
owner={'domain':domain,'initial_state':'shut off','guest_root':root,'account':'OttrinWindowQA','package_source':expected['source_revision'],'archive':str(archive),'archive_sha256':expected['artifacts'][archive.name],'payload_manifest':str(p/'PAYLOAD_GUI_MANIFEST.json'),'scope':'Actual ordinary-account packaged public Copy/Undo workflows and visible listing; original guest files/login values preserved; no physical or full release acceptance'}
(e/'OWNERSHIP.json').write_text(json.dumps(owner,indent=2)+'\n')
def save(): (e/'SESSION.json').write_text(json.dumps(state,indent=2)+'\n')
def cancel(*_):raise KeyboardInterrupt('Owned GUI session cancelled')
for sig in [signal.SIGTERM,signal.SIGINT]:signal.signal(sig,cancel)
def desktop(op):
 args=['python3','scripts/windows-desktop-qa/desktop.py','--evidence-dir',str(e),op if isinstance(op,str) else json.dumps(op)]
 for attempt in range(5):
  q=subprocess.run(args,text=True,capture_output=True,timeout=100)
  with (e/'driver.log').open('a') as log:log.write(q.stdout+q.stderr)
  if q.returncode==75:args[-1]='resume';continue
  if q.returncode:raise RuntimeError('Desktop driver failed: '+q.stdout[-2000:]+q.stderr[-2000:])
  return
 raise TimeoutError('Owned desktop observer did not finish')
def ready():
 for a in range(360):
  try:n.qga('guest-ping');return
  except RuntimeError:time.sleep(.5)
 raise TimeoutError('Guest agent unavailable')
try:
 assert subprocess.check_output(['virsh','-c','qemu:///system','domstate',domain],text=True).strip()=='shut off'
 state['definition_sha256']=hashlib.sha256(subprocess.check_output(['virsh','-c','qemu:///system','dumpxml','--inactive',domain])).hexdigest();assert state['definition_sha256']==json.loads((p/'NATIVE_OWNER.json').read_text())['definition_sha256'];save()
 subprocess.run(['virsh','-c','qemu:///system','start',domain],check=True);started=True;ready()
 # The retained test guest starts its original shell/background tray applications.
 # Refuse any new application before ending this test boot's console session.
 baseline=json.loads((p/'DESKTOP_BASELINE.json').read_text());allowed=sorted({x['Name'].lower() for x in baseline['interactive_processes']})
 for name in ['notepad.exe','winword.exe','excel.exe','ottrin.exe','ottrin-app.exe']:assert name not in allowed
 ps=r'''$ErrorActionPreference='Stop';$allowed=ConvertFrom-Json 'ALLOWED';if((Get-CimInstance Win32_ComputerSystem).UserName-ne'DESKTOP-81GHP8U\campbell'){throw 'Unexpected test console user'};$new=@(Get-CimInstance Win32_Process|Where-Object{$_.SessionId-ne0-and$_.Name.ToLowerInvariant()-notin$allowed});$new=@($new|Where-Object{-not($_.Name.ToLowerInvariant()-in@('userinit.exe','dllhost.exe','slui.exe','mobsync.exe')-and$_.ExecutablePath-in@(('C:\WINDOWS\System32\'+$_.Name),('C:\WINDOWS\SysWOW64\'+$_.Name)))});if($new.Count){throw ('Unclassified console application: '+($new.Name-join','))};$k=[Microsoft.Win32.Registry]::LocalMachine.OpenSubKey('SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon',$false);if('DefaultPassword'-in$k.GetValueNames()){throw 'Existing autologon credential'};$k.Dispose();Add-Type @'
using System;using System.Runtime.InteropServices;public static class TestBootLogoff{[DllImport("wtsapi32.dll",SetLastError=true)]public static extern bool WTSLogoffSession(IntPtr server,int session,bool wait);}
'@
if(-not[TestBootLogoff]::WTSLogoffSession([IntPtr]::Zero,1,$true)){throw 'Test console logoff failed'};@{status='pass';ended_only_this_test_boot_console=$true;no_document_application_observed=$true;original_account_or_profile_removed=$false;new_process_names_refused=$true}|ConvertTo-Json'''.replace('ALLOWED',json.dumps(allowed))
 import base64
 for attempt in range(60):
  response=n.wait(n.start("(Get-CimInstance Win32_ComputerSystem).UserName"))
  user=base64.b64decode(response.get('out-data','')).decode().strip()
  if user==baseline['console_user']:break
  if user:raise RuntimeError('Unexpected active guest user: '+user)
  time.sleep(.5)
 else:raise TimeoutError('Original test console did not become ready')
 result=n.wait(n.start(ps));(e/'CONSOLE_PREPARATION.json').write_bytes(base64.b64decode(result['out-data']))
 state['phase']='prepare';save();prepared=True;desktop('prepare')
 state['phase']='owned-console-boot';save()
 try:n.qga('guest-shutdown',{'mode':'reboot'})
 except RuntimeError:state['reboot_transport_disconnected']=True;save()
 time.sleep(3);ready()
 for a in range(100):
  try:result=n.wait(n.start("if((Get-CimInstance Win32_ComputerSystem).UserName-like'*\\OttrinWindowQA'){Write-Output 'owned-ready'}"))
  except RuntimeError:time.sleep(.5);continue
  if 'owned-ready' in base64.b64decode(result.get('out-data','')).decode():break
  time.sleep(.5)
 else:raise TimeoutError('Owned console did not become ready')
 state['phase']='interactive';state['status']='waiting for owned input';state['next_sequence']=1;save();deadline=time.monotonic()+25*60
 while time.monotonic()<deadline:
  f=e/'NEXT.json'
  if not f.exists():time.sleep(.2);continue
  command=json.loads(f.read_text());f.unlink();assert command['nonce']==nonce and command['sequence']==state['next_sequence']
  if command['op']=='finish':state['status']='requested completion; teardown pending';break
  desktop(command['command'])
  tag=f"{state['next_sequence']:02d}-{command['command']['op']}"
  for ext in ['png','json']:
   f=e/('desktop.'+ext)
   if f.is_file():shutil.copy2(f,e/(tag+'.'+ext))
  state['next_sequence']+=1;save()
 else:raise TimeoutError('Owned GUI session lease expired; automatic teardown')
except BaseException as error:state['status']='failed';state['error']=repr(error);raise
finally:
 try:
  if prepared:
   state['phase']='teardown';save();desktop('cleanup');state['fixtures_removed']=True
 except BaseException as error:state['status']='failed teardown';state['teardown_error']=repr(error);raise
 finally:
  if started:
   try:n.qga('guest-shutdown',{'mode':'powerdown'})
   except RuntimeError:pass
   for a in range(90):
    if subprocess.check_output(['virsh','-c','qemu:///system','domstate',domain],text=True).strip()=='shut off':break
    time.sleep(.5)
   else:raise RuntimeError('Owned GUI guest did not shut down')
   state['guest_off']=True
   assert state['definition_sha256']==hashlib.sha256(subprocess.check_output(['virsh','-c','qemu:///system','dumpxml','--inactive',domain])).hexdigest()
  if state['status']=='requested completion; teardown pending' and state.get('fixtures_removed') and state.get('guest_off'):state['status']='pass'
  save()
