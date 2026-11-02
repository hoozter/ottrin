"""Finite native input/readiness for one owned disposable Windows console."""
from pathlib import Path
import json,os,subprocess,sys,time,importlib.util,re
cohort=Path(__file__).resolve().parent;case=sys.argv[1];assert re.fullmatch('[a-z0-9-]+',case);r=cohort/case;owner=json.loads((r/'OWNER.json').read_text());assert subprocess.check_output(['virsh','-c','qemu:///system','domuuid',owner['domain']],text=True).strip()==owner['uuid']
meta=owner['private_console'];p=Path('/proc')/str(meta['pid']);args=(p/'cmdline').read_bytes().split(b'\0');assert owner['domain'].encode() in args and os.readlink(p/'exe')=='/usr/bin/virt-viewer';values=dict(v.split(b'=',1) for v in (p/'environ').read_bytes().split(b'\0') if b'=' in v);assert os.fsdecode(values[b'DISPLAY'])==meta['display'] and os.fsdecode(values[b'HOME'])==meta['home'] and meta['home'].startswith('/var/tmp/.ottrin-picker-qa-');env={**os.environ,'DISPLAY':meta['display'],'GDK_BACKEND':'x11','EGL_PLATFORM':'x11'};env.pop('XAUTHORITY',None)
q=subprocess.run(['xdotool','search','--onlyvisible','--pid',str(meta['pid'])],env=env,text=True,capture_output=True);assert q.returncode==0,q.stderr;window=q.stdout.split()[-1];action=sys.argv[2]
if action=='click':
 x,y=map(int,sys.argv[3:5]);subprocess.run(['xdotool','windowactivate','--sync',window,'mousemove','--window',window,str(x),str(y),'click','1'],env=env,check=True,timeout=10);time.sleep(.4)
elif action=='key':
 key=sys.argv[3];assert key in ['Escape','Return'];subprocess.run(['xdotool','windowactivate','--sync',window,'key','--clearmodifiers',key],env=env,check=True,timeout=10);time.sleep(.4)
elif action=='shot':
 name=sys.argv[3];assert Path(name).name==name and name.endswith('.png');subprocess.run(['import','-window',window,str(r/name)],env=env,check=True,timeout=10)
elif action in ['ready','ready-msi']:
 assert (r/'desktop-ready.png').is_file(),'Observe the actual ready desktop before signalling'
 spec=importlib.util.spec_from_file_location('guest',Path.cwd()/'.hermes/evidence/windows-20261011/guest_exec.py');g=importlib.util.module_from_spec(spec);spec.loader.exec_module(g);g.DOMAIN=owner['domain']
 code=r'''$ErrorActionPreference='Stop';$root='C:\OttrinOwnedMsixRuntime';$o=Get-Content ($root+'\OWNER.json') -Raw|ConvertFrom-Json;$user=(Get-CimInstance Win32_ComputerSystem).UserName;if($user-ne($env:COMPUTERNAME+'\'+$o.account)){throw 'Console is not the owned temporary user'};$value=@{sid=$o.sid;nonce=$o.nonce;method='Observed first-login privacy selections through the owned private software console; optional collection declined; no security or OOBE policy changes'};$value|ConvertTo-Json -Compress|Set-Content -Encoding UTF8 ($root+'\results\DESKTOP_READY.json');$value|ConvertTo-Json -Compress'''
 if action=='ready-msi':
  code=code.replace("$root='C:\\OttrinOwnedMsixRuntime';$o=Get-Content ($root+'\\OWNER.json')","$root='C:\\OttrinMsiCleanClientResults';$o=Get-Content ($root+'\\OWNER.json')").replace("($root+'\\results\\DESKTOP_READY.json')","($o.root+'\\results\\DESKTOP_READY.json')")
 status=g.execute(code,30);assert status==0;(r/'DESKTOP_SIGNAL.json').write_text(json.dumps({'status':'sent to owned user fixture','domain_uuid':owner['uuid'],'observed_frame':'desktop-ready.png','scope':'Actual ordinary first-login setup, no security policy override'},indent=2)+'\n')
else:raise ValueError('Unknown console action')
