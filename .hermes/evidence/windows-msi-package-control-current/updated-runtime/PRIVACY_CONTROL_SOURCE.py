"""Finite native input/readiness for one owned disposable Windows console."""
from pathlib import Path
import json,os,subprocess,sys,time,importlib.util,re
r=Path(sys.argv[1]).resolve();assert r.is_relative_to(Path.cwd()/'.hermes/evidence') and re.fullmatch('[a-z0-9-]+',r.name);owner=json.loads((r/'OWNER.json').read_text());assert subprocess.check_output(['virsh','-c','qemu:///system','domuuid',owner['domain']],text=True).strip()==owner['uuid']
meta=owner['private_console'];p=Path('/proc')/str(meta['pid']);args=(p/'cmdline').read_bytes().split(b'\0');assert owner['domain'].encode() in args and os.readlink(p/'exe')=='/usr/bin/virt-viewer';values=dict(v.split(b'=',1) for v in (p/'environ').read_bytes().split(b'\0') if b'=' in v);assert os.fsdecode(values[b'DISPLAY'])==meta['display'] and os.fsdecode(values[b'HOME'])==meta['home'] and meta['home'].startswith('/var/tmp/.ottrin-picker-qa-');env={**os.environ,'DISPLAY':meta['display'],'GDK_BACKEND':'x11','EGL_PLATFORM':'x11'};env.pop('XAUTHORITY',None)
q=subprocess.run(['xdotool','search','--onlyvisible','--pid',str(meta['pid'])],env=env,text=True,capture_output=True);assert q.returncode==0,q.stderr;window=q.stdout.split()[-1];action=sys.argv[2]
if action=='click':
 x,y=map(int,sys.argv[3:5]);subprocess.run(['xdotool','windowactivate','--sync',window,'mousemove','--window',window,str(x),str(y),'click','1'],env=env,check=True,timeout=10);time.sleep(.4)
elif action=='scroll':
 x,y,count=map(int,sys.argv[3:6]);assert 1<=abs(count)<=10
 geometry=subprocess.check_output(['xdotool','getwindowgeometry','--shell',window],env=env,text=True,timeout=10);bounds=dict(line.split('=',1) for line in geometry.splitlines() if '=' in line);assert 0<=x<int(bounds['WIDTH']) and 0<=y<int(bounds['HEIGHT'])
 subprocess.run(['xdotool','windowactivate','--sync',window,'mousemove','--window',window,str(x),str(y),'click','--repeat',str(abs(count)),'--delay','100','5' if count>0 else '4'],env=env,check=True,timeout=10);time.sleep(.4)
elif action=='key':
 key=sys.argv[3];assert key in ['Escape','Return'];subprocess.run(['xdotool','windowactivate','--sync',window,'key','--clearmodifiers',key],env=env,check=True,timeout=10);time.sleep(.4)
elif action=='shot':
 name=sys.argv[3];assert Path(name).name==name and name.endswith('.png');subprocess.run(['import','-window',window,str(r/name)],env=env,check=True,timeout=10)
elif action in ['finish-msix','finish-application']:
 # The producer still requires an observed normal desktop. Complete only these
 # known first-login pages through native input; never change OOBE/security policy.
 import hashlib
 steps=[]
 def own(*args):subprocess.run([sys.executable,str(Path(__file__).resolve()),str(r),*map(str,args)],check=True,timeout=40)
 own('key','Escape')
 # Editions can omit privacy pages. Act once only on a recognized, observed
 # page; a transition timeout never repeats input or restarts the guest.
 allowed={'use your location':550,'find my device':550,'improve inking':550,'personalized offers':610}
 seen=set();deadline=time.monotonic()+600
 while True:
  filename='privacy-accepted-'+str(len(steps))+'.png';own('shot',filename)
  text=subprocess.check_output(['tesseract',str(r/filename),'stdout','--psm','11'],stderr=subprocess.DEVNULL,text=True,timeout=10).casefold()
  if 'recycle bin' in text or all(label in text for label in ['pinned','recommended','file explorer']):break
  matched=[heading for heading in allowed if heading in text]
  assert len(matched)<=1,'Ambiguous privacy page; no input sent'
  if matched and matched[0] not in seen:
   heading=matched[0];y=allowed[heading];seen.add(heading)
   steps.append({'heading':heading,'observed_sha256':hashlib.sha256((r/filename).read_bytes()).hexdigest(),'selection':'No','input':[[900,y],[1080,737]]})
   own('click',900,y);own('click',1080,737);deadline=time.monotonic()+45
  assert time.monotonic()<deadline,'Known privacy page transition or normal desktop did not appear; no repeated input'
  time.sleep(.5)
 own('key','Escape');own('shot','desktop-ready.png');own('ready-msi' if action=='finish-application' else 'ready')
 (r/'PRIVACY_STEPS.json').write_text(json.dumps({'status':'pass','scope':'Known first-login pages observed by OCR and completed with native input on owned private console; optional collection declined; no policy bypass','domain_uuid':owner['uuid'],'steps':steps,'desktop_observed':True},indent=2)+'\n')
elif action in ['ready','ready-msi']:
 assert (r/'desktop-ready.png').is_file(),'Observe the actual ready desktop before signalling'
 spec=importlib.util.spec_from_file_location('guest',Path.cwd()/'.hermes/evidence/windows-20261011/guest_exec.py');g=importlib.util.module_from_spec(spec);spec.loader.exec_module(g);g.DOMAIN=owner['domain']
 code=r'''$ErrorActionPreference='Stop';$root='C:\OttrinOwnedMsixRuntime';$o=Get-Content ($root+'\OWNER.json') -Raw|ConvertFrom-Json;$user=(Get-CimInstance Win32_ComputerSystem).UserName;if($user-ne($env:COMPUTERNAME+'\'+$o.account)){throw 'Console is not the owned temporary user'};$value=@{sid=$o.sid;nonce=$o.nonce;method='Observed the owned account desktop through the private software console; privacy choices, when required, are recorded separately; no security or OOBE policy changes'};$value|ConvertTo-Json -Compress|Set-Content -Encoding UTF8 ($root+'\results\DESKTOP_READY.json');$value|ConvertTo-Json -Compress'''
 if action=='ready-msi':
  code=code.replace("$root='C:\\OttrinOwnedMsixRuntime';$o=Get-Content ($root+'\\OWNER.json')","$root='C:\\OttrinMsiCleanClientResults';$o=Get-Content ($root+'\\OWNER.json')").replace("($root+'\\results\\DESKTOP_READY.json')","($o.root+'\\results\\DESKTOP_READY.json')")
 status=g.execute(code,30);assert status==0;(r/'DESKTOP_SIGNAL.json').write_text(json.dumps({'status':'sent to owned user fixture','domain_uuid':owner['uuid'],'observed_frame':'desktop-ready.png','scope':'Actual owned account desktop; actor independently verifies its token; no security policy override'},indent=2)+'\n')
else:raise ValueError('Unknown console action')
