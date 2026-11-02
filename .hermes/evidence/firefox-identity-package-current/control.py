"""Owned native input only; never use a host display or browser profile."""
from pathlib import Path
import argparse,json,os,subprocess,time,sys
parser=argparse.ArgumentParser();parser.add_argument('stage',choices=['open','save']);parser.add_argument('--output',type=Path,required=True);args=parser.parse_args();r=args.output.resolve();assert r.is_relative_to(Path.cwd()/'.hermes/evidence');s=json.loads((r/'desktop/SESSION.json').read_text());env={**os.environ,**s['environment']};assert env['DISPLAY']==s['display'] and env['HOME'].startswith('/var/tmp/.ottrin-picker-qa-')
def run(*args):return subprocess.run(list(args),env=env,text=True,capture_output=True,check=True,timeout=10).stdout
if args.stage=='open':
 if s['browser']=='falkon-flatpak':
  run('xdotool','windowactivate','--sync',s['chooser_window'],'mousemove','--window',s['chooser_window'],'110','560');time.sleep(.15);run('xdotool','mousedown','1');time.sleep(.1);run('xdotool','mouseup','1');time.sleep(.2)
  run('import','-window',s['chooser_window'],str(r/'desktop/open-filter-menu.png'))
  run('xdotool','mousemove','--window',s['chooser_window'],'120','500');time.sleep(.15);run('xdotool','mousedown','1');time.sleep(.1);run('xdotool','mouseup','1');time.sleep(.4)
  run('import','-window',s['chooser_window'],str(r/'desktop/open-selected-filter.png'))
 run('xdotool','windowactivate','--sync',s['chooser_window'],'mousemove','--window',s['chooser_window'],'300','183','click','1','mousemove','--window',s['chooser_window'],'900','606','click','1');time.sleep(.6)
 run('import','-window',s['browser_window'],str(r/'desktop/opened-source.png'))
 run('xdotool','windowactivate','--sync',s['browser_window'],'windowfocus','--sync',s['browser_window'],'mousemove','--window',s['browser_window'],'500','300');time.sleep(.15);run('xdotool','click','1');time.sleep(.3)
 run('xdotool','windowactivate','--sync',s['browser_window']);time.sleep(.5);assert run('xdotool','getactivewindow').strip()==s['browser_window'];run('xdotool','key','--clearmodifiers','ctrl+s')
 deadline=time.monotonic()+25
 while time.monotonic()<deadline:
  q=subprocess.run(['xdotool','search','--onlyvisible','--name','Save'],env=env,text=True,capture_output=True)
  if q.stdout.strip():break
  if s['browser']=='falkon-flatpak':
   options=subprocess.run(['xdotool','search','--onlyvisible','--name','^Opening original.txt$'],env=env,text=True,capture_output=True).stdout.split()
   if options:
    option=options[-1];run('import','-window',option,str(r/'desktop/download-options.png'));run('xdotool','windowactivate','--sync',option);time.sleep(.2);run('xdotool','key','--clearmodifiers','Return')
  time.sleep(.1)
 if not q.stdout.strip():
  (r/'desktop/SAVE_TIMEOUT_WINDOWS.txt').write_text(run('xwininfo','-root','-tree'));run('import','-window','root',str(r/'desktop/save-timeout.png'))
 assert q.stdout.strip(),'Browser Save chooser did not map';s['save_window']=q.stdout.split()[-1];(r/'desktop/SESSION.json').write_text(json.dumps(s,indent=2)+'\n');time.sleep(.3);run('import','-window',s['save_window'],str(r/'desktop/save.png'))
elif args.stage=='save':
 w=s['save_window'];pid=int(run('xdotool','getwindowpid',w).strip());actual=Path(os.readlink('/proc/'+str(pid)+'/exe'));assert actual.name=='ottrin' and '.mount_Ottrin' in str(actual)
 run('xdotool','windowactivate','--sync',w,'key','ctrl+l','type','--clearmodifiers',str(Path(s['fixture'])/'saved'));run('xdotool','key','Return');time.sleep(.4);run('import','-window',w,str(r/'desktop/save-destination.png'));run('xdotool','mousemove','--window',w,'900','606','click','1')
 dest=Path(s['fixture'])/'saved/original.txt';source=Path(s['fixture'])/'original.txt';deadline=time.monotonic()+20
 while not dest.exists() or dest.read_bytes()!=source.read_bytes():assert time.monotonic()<deadline,'Browser did not finish writing the chosen save path';time.sleep(.1)
 assert dest.read_bytes()==source.read_bytes()==bytes.fromhex(s['payload_hex'])
 time.sleep(.3);run('import','-window',s['browser_window'],str(r/'desktop/saved-browser.png'))
 import hashlib
 (r/'CONTROLLER_RESULT.json').write_text(json.dumps({'status':'pass','actual_browser':s['browser'],'open_save':'Actual native Open and Save dialogs both owned by the delivered AppImage chooser; Open displays the outside-sandbox selected document; Save writes exact synthetic bytes'+' from the opened local text document','source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'saved_sha256':hashlib.sha256(dest.read_bytes()).hexdigest(),'chooser_backend_pid':pid,'no_index_started':not (Path(s['profile'])/'runtime/ottrin/index-v1.sock').exists(),'identity_scope':'Parent runner verifies actual installed browser attribution independently from the frontend log; this controller verifies Open/Save bytes and chooser ownership.'},indent=2)+'\n')
else:raise ValueError('Unknown native stage')
