"""Bounded native-parent and OS-close checks in the canonical owned desktop."""
from pathlib import Path
import argparse,hashlib,importlib.util,json,time,os
r=Path.cwd();e=Path(__file__).resolve().parent
p=argparse.ArgumentParser();p.add_argument('--binary',type=Path,required=True);p.add_argument('--case',choices=['before','after'],required=True);a=p.parse_args();binary=a.binary.resolve();out=e/('x11-'+a.case);assert not out.exists()
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');desktop=importlib.util.module_from_spec(spec);spec.loader.exec_module(desktop)
report={'binary':str(binary),'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'scope':'Actual X11 foreign parent, parent loss and native WM close; isolated source GUI, no Windows/Wayland/package claim'}
def wait_window(d,process,title):
 deadline=time.monotonic()+20
 while time.monotonic()<deadline:
  assert process.poll() is None,('Early exit',process.returncode)
  windows=desktop.run(['xdotool','search','--onlyvisible','--name',title],d.env,False).stdout.split()
  if windows:return windows[-1]
  time.sleep(.05)
 raise TimeoutError('Owned window did not map')
def picker(d,request,name):
 app=d.spawn([str(binary),'picker'],name+'.stderr',stdin=True,output_name=name+'.response.json');app.stdin.write(json.dumps(request));app.stdin.close();return app
try:
 with desktop.OwnedDesktop(out) as d:
  fixture=d.profile/'fixture';fixture.mkdir();f=fixture/'original.txt';f.write_bytes(b'original file\n');original=hashlib.sha256(f.read_bytes()).hexdigest()
  parent=d.spawn(['xmessage','-title','Ottrin foreign caller QA','-buttons','Close:0','Owned external caller'],'parent.log');parent_window=wait_window(d,parent,'^Ottrin foreign caller QA$')
  request={'title':'Owned parent lifecycle QA','parent_window':f'x11:{int(parent_window):x}','initial_directory':{'encoding':'unix-bytes','units_hex':os.fsencode(fixture).hex()}}
  app=picker(d,request,'parent');window=wait_window(d,app,'Owned parent lifecycle QA');time.sleep(.3)
  transient=desktop.run(['xprop','-id',window,'WM_TRANSIENT_FOR','_NET_WM_WINDOW_TYPE'],d.env).stdout;(out/'NATIVE_PROPERTIES.txt').write_text(transient);attached=f'0x{int(parent_window):x}' in transient
  report['attached']=attached
  if a.case=='after':assert attached and '_NET_WM_WINDOW_TYPE_DIALOG' in transient
  else:assert not attached
  desktop.run(['import','-window',window,str(out/'picker.png')],d.env)
  parent.terminate();parent.wait(timeout=5)
  if a.case=='after':app.wait(timeout=5);assert app.returncode==0
  else:
   time.sleep(.5);assert app.poll() is None;desktop.run(['xdotool','windowactivate','--sync',window],d.env);desktop.run(['xdotool','key','--clearmodifiers','Escape'],d.env);app.wait(timeout=5)
  response=json.loads((out/'parent.response.json').read_text());assert not response['accepted'] and not response['paths'];report['parent_loss_cancelled']=a.case=='after'
  pref=d.profile/'config/ottrin/picker.json'
  if pref.exists():pref.unlink()
  request['parent_window']='';request['title']='Owned native close QA';app=picker(d,request,'close');window=wait_window(d,app,'Owned native close QA');time.sleep(.3)
  # Send the window-manager close request, rather than click Ottrin's own titlebar.
  desktop.run(['xdotool','windowactivate','--sync',window],d.env);desktop.run(['xdotool','key','--clearmodifiers','alt+F4'],d.env);app.wait(timeout=5);assert app.returncode==0
  response=json.loads((out/'close.response.json').read_text());assert not response['accepted'];report['native_close_preferences_saved']=pref.exists()
  assert pref.exists()==(a.case=='after')
  if pref.exists():
   saved=json.loads(pref.read_text());assert saved['last_directory']['units_hex']==os.fsencode(fixture).hex();report['saved_preferences']=saved
  assert hashlib.sha256(f.read_bytes()).hexdigest()==original;assert not (d.profile/'runtime/ottrin/index-v1.sock').exists();report.update(original_bytes_preserved=True,no_index_started=True,status='pass' if a.case=='after' else 'reproduced missing parent ownership and native-close persistence')
finally:
 report['teardown']=json.loads((out/'TEARDOWN.json').read_text()) if (out/'TEARDOWN.json').exists() else None
 (out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
