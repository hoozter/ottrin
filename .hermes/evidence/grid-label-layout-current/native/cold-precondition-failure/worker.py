"""No-input settled-count observation and exact-source Grid visual capture."""
from pathlib import Path
import hashlib,importlib.util,json,os,signal,subprocess,time,traceback
from PIL import ImageGrab
base=Path.home();out=base/'results';report={'status':'running','startup_samples':[],'grid_cases':[]};app=None;wm=None
spec=importlib.util.spec_from_file_location('qa',base/'linux-desktop-qa.py');q=importlib.util.module_from_spec(spec);spec.loader.exec_module(q)
def run(*args):
 p=subprocess.run(args,capture_output=True,text=True,timeout=45)
 assert p.returncode==0,(args,p.returncode,p.stdout,p.stderr)
 return p.stdout
def capture(name):ImageGrab.grab(xdisplay=os.environ['DISPLAY']).save(out/(name+'.png'))
def launch(exe,name):
 global app
 log=(out/(name+'.log')).open('w');app=subprocess.Popen([exe,str(base/'fixtures')],stdout=log,stderr=log);log.close();q.target_process_id=app.pid
 q.wait_for(q.ottrin_app,'owned first GUI launch',timeout=30)
 window=q.wait_for(lambda:q.run('xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','^Ottrin$',capture=True,check=False).stdout.strip(),'main window')
 assert len(window.split())==1
 return window
try:
 assert os.environ.get('XDG_SESSION_ID')
 run('dbus-update-activation-environment','--systemd','DISPLAY','XDG_SESSION_TYPE','GTK_MODULES','NO_AT_BRIDGE')
 run('gsettings','set','org.gnome.desktop.interface','toolkit-accessibility','true')
 wm=subprocess.Popen(['openbox'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 run('/usr/bin/ottrin','index','service-install')
 run('systemctl','--user','disable','ottrin-index.service');run('systemctl','--user','stop','ottrin-index.service')
 assert not (Path(os.environ['XDG_RUNTIME_DIR'])/'ottrin/index-v1.sock').exists()
 window=launch('/usr/bin/ottrin','cold-miller');start=time.monotonic()
 # Sampling uses accessibility reads and screen captures, never pointer/key events.
 for seconds in [0,2,10,30]:
  remaining=start+seconds-time.monotonic()
  if remaining>0:time.sleep(remaining)
  nodes=q.visible_a11y_nodes();(out/f'cold-{seconds}.a11y.txt').write_text('\n'.join(nodes)+'\n');capture(f'cold-{seconds}')
  report['startup_samples'].append({'seconds_after_window':time.monotonic()-start,'item_labels':[line for line in nodes if ' items' in line]})
 q.xkey(window,'alt+F4');assert app.wait(timeout=15)==0
 for theme,scale in [('Dark',1.0),('Light',1.0),('Dark',.75),('Dark',1.85)]:
  pref=base/'.config/ottrin/config.json';config=json.loads(pref.read_text());config.update(theme=theme,default_view_mode='Grid',grid_view_scale=scale);pref.write_text(json.dumps(config))
  label=f'grid-{theme.lower()}-{scale}';window=launch(str(base/'grid-ottrin-app'),label)
  q.wait_for(lambda:q.visible_entry('owned-0005.txt'),'Grid fixture populated')
  time.sleep(2);capture(label);nodes=q.visible_a11y_nodes();(out/(label+'.a11y.txt')).write_text('\n'.join(nodes)+'\n')
  actual=hashlib.sha256(Path(f'/proc/{app.pid}/exe').read_bytes()).hexdigest();assert actual==json.loads((base/'GUI_INPUT.json').read_text())['gui_sha256']
  report['grid_cases'].append({'theme':theme,'scale':scale,'gui_sha256':actual,'capture':label+'.png'})
  q.xkey(window,'alt+F4');assert app.wait(timeout=15)==0
 assert all((base/'fixtures'/f'owned-{i:04}.txt').read_text()==f'owned{i}\n' for i in range(1000))
 run('systemctl','--user','stop','ottrin-index.service');report['status']='pass'
except BaseException as error:
 report.update(status='fail',error=repr(error),traceback=traceback.format_exc())
 try:capture('failure')
 except Exception:pass
finally:
 if app is not None and app.poll() is None:
  app.terminate()
  try:app.wait(timeout=10)
  except subprocess.TimeoutExpired:app.kill();app.wait()
 (out/'WORKER_RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
while True:signal.pause()
