"""Actual package filename/density matrix, settled footer and cooperating picker."""
from pathlib import Path
import hashlib,importlib.util,json,os,signal,subprocess,time,traceback
from PIL import ImageGrab
base=Path.home();out=base/'results';report={'status':'running','startup_samples':[],'grid_cases':[],'picker_cases':[],'miller_cases':[],'list_cases':[],'appearance_cases':[]};app=None;wm=None;display=None
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
 display=subprocess.Popen(['Xvfb',':99','-screen','0','1920x1200x24','-nolisten','tcp'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 os.environ['DISPLAY']=':99';os.environ['WINIT_X11_SCALE_FACTOR']='1.0'
 q.wait_for(lambda:q.run('xdotool','getdisplaygeometry',capture=True,check=False).stdout.strip()=='1920 1200','owned1920x1200 display')
 wm=subprocess.Popen(['openbox'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 def wm_ready():
  assert wm.poll() is None, ('Openbox exited',wm.returncode)
  return '_NET_SUPPORTING_WM_CHECK(WINDOW)' in q.run('xprop','-root','_NET_SUPPORTING_WM_CHECK',capture=True,check=False).stdout
 q.wait_for(wm_ready,'owned EWMH window manager before application startup',timeout=15)
 # Reproduce the previously failing installed-service/warm-index precondition.
 report['service_install']=run('/usr/bin/ottrin','index','service-install')
 assert run('systemctl','--user','is-enabled','ottrin-index.service').strip()=='enabled'
 import socket,struct
 def diagnostics():
  body=json.dumps({'version':2,'request':{'type':'status'}}).encode()
  with socket.socket(socket.AF_UNIX) as client:
   client.settimeout(3);client.connect(str(Path(os.environ['XDG_RUNTIME_DIR'])/'ottrin/index-v1.sock'));client.sendall(struct.pack('!I',len(body))+body)
   def exact(n):
    value=b''
    while len(value)<n:
     part=client.recv(n-len(value));assert part;value+=part
    return value
   count=struct.unpack('!I',exact(4))[0];assert count<1024*1024
   return json.loads(exact(count))['response']['diagnostics']
 def ready():
  try:
   d=diagnostics();return d if d['status']=='Ready' and d['indexed_items']>=1002 else None
  except (OSError,KeyError):return None
 report['pre_gui_warm_index']=q.wait_for(ready,'installed daemon ready before GUI',timeout=60)
 image=base/json.loads((base/'APPIMAGE_INPUT.json').read_text())['name']
 window=launch(str(image),'cold-miller');start=time.monotonic()
 # Sampling uses accessibility reads and screen captures, never pointer/key events.
 for seconds in [0,2,10,30]:
  remaining=start+seconds-time.monotonic()
  if remaining>0:time.sleep(remaining)
  nodes=q.visible_a11y_nodes();(out/f'cold-{seconds}.a11y.txt').write_text('\n'.join(nodes)+'\n');capture(f'cold-{seconds}')
  report['startup_samples'].append({'seconds_after_window':time.monotonic()-start,'item_labels':[line for line in nodes if ' items' in line]})
 q.xkey(window,'alt+F4');assert app.wait(timeout=15)==0
 for mode,key in [('Miller','miller_cases'),('List','list_cases')]:
  for theme in ['Dark','Light']:
   for scale in [.75,1.0,1.85]:
    pref=base/'.config/ottrin/config.json';config=json.loads(pref.read_text());config.update(theme=theme,theme_preset='Ottrin',active_custom_theme=None,default_view_mode=mode,miller_view_scale=1.0,list_view_scale=1.0,grid_view_scale=1.0)
    config['miller_view_scale' if mode=='Miller' else 'list_view_scale']=scale;pref.write_text(json.dumps(config))
    label=f'{mode.lower()}-{theme.lower()}-{scale}';window=launch(str(image),label)
    row=q.wait_for(lambda:q.visible_entry('00-WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW.txt'),'actual wide filename row')
    bounds=row.queryComponent().getExtents(q.pyatspi.DESKTOP_COORDS)
    time.sleep(1);capture(label);(out/(label+'.a11y.txt')).write_text('\n'.join(q.visible_a11y_nodes())+'\n')
    actual=hashlib.sha256(Path(f'/proc/{app.pid}/exe').read_bytes()).hexdigest();assert actual==json.loads((base/'APPIMAGE_INPUT.json').read_text())['gui_sha256']
    report[key].append({'theme':theme,'scale':scale,'gui_sha256':actual,'capture':label+'.png','row_height':bounds.height,'row_width':bounds.width})
    q.xkey(window,'alt+F4');assert app.wait(timeout=15)==0
    saved=json.loads(pref.read_text());assert saved['default_view_mode']==mode and saved['miller_view_scale' if mode=='Miller' else 'list_view_scale']==scale
 for key in ['miller_cases','list_cases']:
  for theme in ['Dark','Light']:
   cases=sorted((case for case in report[key] if case['theme']==theme),key=lambda case:case['scale'])
   heights=[case['row_height'] for case in cases];assert len(cases)==3 and 0<heights[0]<heights[1]<heights[2],(key,theme,heights)
 for theme,scale in [(theme,scale) for theme in ['Dark','Light'] for scale in [.75,1.0,1.85]]:
  pref=base/'.config/ottrin/config.json';config=json.loads(pref.read_text());config.update(theme=theme,default_view_mode='Grid',miller_view_scale=1.0,list_view_scale=1.0,grid_view_scale=scale);pref.write_text(json.dumps(config))
  label=f'grid-{theme.lower()}-{scale}';window=launch(str(image),label)
  q.wait_for(lambda:q.visible_entry('owned-0005.txt'),'Grid fixture populated')
  q.run('xdotool','mousemove','5','5');time.sleep(2);capture(label);nodes=q.visible_a11y_nodes();(out/(label+'.a11y.txt')).write_text('\n'.join(nodes)+'\n')
  actual=hashlib.sha256(Path(f'/proc/{app.pid}/exe').read_bytes()).hexdigest();assert actual==json.loads((base/'APPIMAGE_INPUT.json').read_text())['gui_sha256']
  report['grid_cases'].append({'theme':theme,'scale':scale,'gui_sha256':actual,'capture':label+'.png'})
  q.xkey(window,'alt+F4');assert app.wait(timeout=15)==0
 for theme in ['Dark','Light']:
  pref=base/'.config/ottrin/config.json';config=json.loads(pref.read_text());config.update(theme=theme,grid_view_scale=1.0);pref.write_text(json.dumps(config))
  label='picker-grid-'+theme.lower()
  with (out/(label+'.stderr')).open('w') as log:
   app=subprocess.Popen([str(image),'picker'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=log,text=True)
  request={'mode':'open','title':'Owned Grid picker QA','initial_directory':{'encoding':'unix-bytes','units_hex':str(base/'fixtures').encode().hex()}}
  app.stdin.write(json.dumps(request));app.stdin.close();q.target_process_id=app.pid
  q.wait_for(q.ottrin_app,'actual packaged picker',timeout=30)
  window=q.wait_for(lambda:q.run('xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','Owned Grid picker QA',capture=True,check=False).stdout.strip(),'owned picker window')
  assert len(window.split())==1
  q.click_accessible_name('Grid view','push button')
  q.wait_for(lambda:q.visible_entry('owned-0005.txt'),'picker Grid fixture visible')
  q.run('xdotool','mousemove','5','5');time.sleep(1);capture(label)
  (out/(label+'.a11y.txt')).write_text('\n'.join(q.visible_a11y_nodes())+'\n')
  actual=hashlib.sha256(Path(f'/proc/{app.pid}/exe').read_bytes()).hexdigest();assert actual==json.loads((base/'APPIMAGE_INPUT.json').read_text())['gui_sha256']
  q.xkey(window,'Escape');assert app.wait(timeout=15)==0
  reply=json.loads(app.stdout.read());assert reply['accepted'] is False and reply['paths']==[],reply
  report['picker_cases'].append({'theme':theme,'capture':label+'.png','cancelled':True,'gui_sha256':actual})
 os.environ['WINIT_X11_SCALE_FACTOR']='1.5'
 for theme in ['Dark','Light']:
  pref=base/'.config/ottrin/config.json';config=json.loads(pref.read_text());config.update(theme=theme,default_view_mode='Miller',miller_view_scale=1.0,list_view_scale=1.0,grid_view_scale=1.0,window_size=[800,500]);pref.write_text(json.dumps(config))
  label='miller150-'+theme.lower();window=launch(str(image),label)
  geometry=q.run('xdotool','getwindowgeometry','--shell',window,capture=True).stdout
  width=int(next(line.split('=',1)[1] for line in geometry.splitlines() if line.startswith('WIDTH=')))
  assert abs(width-1200)<=8,('Backend150% scale not applied',geometry)
  q.wait_for(lambda:q.visible_entry('00-WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW.txt'),'scaled wide Miller row visible')
  time.sleep(1);capture(label);(out/(label+'.a11y.txt')).write_text('\n'.join(q.visible_a11y_nodes())+'\n')
  actual=hashlib.sha256(Path(f'/proc/{app.pid}/exe').read_bytes()).hexdigest();assert actual==json.loads((base/'APPIMAGE_INPUT.json').read_text())['gui_sha256']
  report['miller_cases'].append({'theme':theme,'gui_sha256':actual,'capture':label+'.png','physical_width':width,'backend_scale':1.5})
  q.xkey(window,'alt+F4');assert app.wait(timeout=15)==0
 assert all((base/'fixtures'/f'owned-{i:04}.txt').read_text()==f'owned{i}\n' for i in range(1000))
 report['status']='pass'
 report['cold_scope']='Fresh account with installed/enabled daemon confirmed Ready before first GUI; no input during footer samples. No raw systemctl-stop precondition.'
except BaseException as error:
 report.update(status='fail',error=repr(error),traceback=traceback.format_exc())
 try:capture('failure')
 except Exception:pass
finally:
 if app is not None and app.poll() is None:
  app.terminate()
  try:app.wait(timeout=10)
  except subprocess.TimeoutExpired:app.kill();app.wait()
 for process in [wm,display]:
  if process is not None and process.poll() is None:
   process.terminate()
   try:process.wait(timeout=10)
   except subprocess.TimeoutExpired:process.kill();process.wait()
 (out/'WORKER_RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
while True:signal.pause()
