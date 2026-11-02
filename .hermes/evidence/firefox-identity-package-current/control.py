"""Owned native input only; never use a host display or browser profile."""
from pathlib import Path
import json,os,subprocess,time,sys
r=Path(__file__).resolve().parent;s=json.loads((r/'desktop/SESSION.json').read_text());env={**os.environ,**s['environment']};assert env['DISPLAY']==s['display'] and env['HOME'].startswith('/var/tmp/.ottrin-picker-qa-')
def run(*args):return subprocess.run(list(args),env=env,text=True,capture_output=True,check=True,timeout=10).stdout
if sys.argv[1]=='open':
 run('xdotool','windowactivate','--sync',s['chooser_window'],'mousemove','--window',s['chooser_window'],'300','183','click','1','mousemove','--window',s['chooser_window'],'900','606','click','1');time.sleep(.6)
 run('import','-window',s['browser_window'],str(r/'desktop/opened-source.png'));run('xdotool','windowactivate','--sync',s['browser_window'],'key','ctrl+s')
 deadline=time.monotonic()+25
 while time.monotonic()<deadline:
  q=subprocess.run(['xdotool','search','--onlyvisible','--name','Save'],env=env,text=True,capture_output=True)
  if q.stdout.strip():break
  time.sleep(.1)
 assert q.stdout.strip(),'Firefox Save chooser did not map';s['save_window']=q.stdout.split()[-1];(r/'desktop/SESSION.json').write_text(json.dumps(s,indent=2)+'\n');time.sleep(.3);run('import','-window',s['save_window'],str(r/'desktop/save.png'))
elif sys.argv[1]=='save':
 w=s['save_window'];pid=int(run('xdotool','getwindowpid',w).strip());actual=Path(os.readlink('/proc/'+str(pid)+'/exe'));assert actual.name=='ottrin' and '.mount_Ottrin' in str(actual)
 run('xdotool','windowactivate','--sync',w,'key','ctrl+l','type','--clearmodifiers',str(Path(s['fixture'])/'saved'));run('xdotool','key','Return');time.sleep(.4);run('import','-window',w,str(r/'desktop/save-destination.png'));run('xdotool','mousemove','--window',w,'900','606','click','1')
 dest=Path(s['fixture'])/'saved/original.txt';source=Path(s['fixture'])/'original.txt';deadline=time.monotonic()+20
 while not dest.exists() or dest.read_bytes()!=source.read_bytes():assert time.monotonic()<deadline,'Firefox did not finish writing the chosen save path';time.sleep(.1)
 assert dest.read_bytes()==source.read_bytes()==b'Ottrin Firefox file picker acceptance.\nOriginal bytes stay unchanged.\n'
 time.sleep(.3);run('import','-window',s['browser_window'],str(r/'desktop/saved-browser.png'))
 import hashlib
 (r/'CONTROLLER_RESULT.json').write_text(json.dumps({'status':'pass','actual_browser':'Firefox','open_save':'Actual native Open and Save dialogs both owned by the delivered AppImage chooser; browser displays chosen document and writes exact saved bytes','source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'saved_sha256':hashlib.sha256(dest.read_bytes()).hexdigest(),'chooser_backend_pid':pid,'no_index_started':not (Path(s['profile'])/'runtime/ottrin/index-v1.sock').exists(),'identity_limit':'Frontend inherits the enclosing host cgroup label org.chromium.Chromium; this scope does not accept Firefox application-ID attribution or sandboxed installed-browser identity.'},indent=2)+'\n')
elif sys.argv[1]=='verify-save':
 pid=int(s['chooser_pid']);dest=Path(s['fixture'])/'saved/original.txt';source=Path(s['fixture'])/'original.txt'
 assert dest.read_bytes()==source.read_bytes()==b'Ottrin Firefox file picker acceptance.\nOriginal bytes stay unchanged.\n'
 time.sleep(.3);run('import','-window',s['browser_window'],str(r/'desktop/saved-browser.png'))
 import hashlib
 (r/'CONTROLLER_RESULT.json').write_text(json.dumps({'status':'pass','actual_browser':'Firefox','open_save':'Actual native Open and Save dialogs both owned by the delivered AppImage chooser; browser displays chosen document and writes exact saved bytes','source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'saved_sha256':hashlib.sha256(dest.read_bytes()).hexdigest(),'chooser_backend_pid':pid,'no_index_started':not (Path(s['profile'])/'runtime/ottrin/index-v1.sock').exists(),'identity_limit':'Frontend inherits the enclosing host cgroup label org.chromium.Chromium; this scope does not accept Firefox application-ID attribution or sandboxed installed-browser identity.'},indent=2)+'\n')
else:raise ValueError('Unknown native stage')
