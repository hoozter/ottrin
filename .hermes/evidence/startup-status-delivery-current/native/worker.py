"""Actual packaged Services startup choices, runtime independence and controlled probe failure."""
from pathlib import Path
import hashlib,importlib.util,json,os,signal,socket,struct,subprocess,time,traceback
from PIL import ImageGrab
B=Path.home();O=B/'results';app=None;wm=None;display=None
report={'status':'running','cases':[],'scope':'Owned guest AppImage/FUSE, installed enabled service, controlled software X11; query failure is explicitly simulated.'}
spec=importlib.util.spec_from_file_location('qa',B/'linux-desktop-qa.py');q=importlib.util.module_from_spec(spec);spec.loader.exec_module(q)
def run(*args):
 p=subprocess.run(args,capture_output=True,text=True,timeout=45)
 assert p.returncode==0,(args,p.returncode,p.stdout,p.stderr)
 return p.stdout
def enabled():
 p=subprocess.run(['/usr/bin/systemctl','--user','is-enabled','ottrin-index.service'],capture_output=True,text=True,timeout=10)
 assert (p.returncode,p.stdout.strip()) in [(0,'enabled'),(1,'disabled')],(p.returncode,p.stdout,p.stderr)
 return p.stdout.strip()=='enabled'
def unit():return dict(line.split('=',1) for line in run('/usr/bin/systemctl','--user','show','ottrin-index.service','-p','MainPID','-p','ActiveState').splitlines())
def diagnostic():
 body=json.dumps({'version':2,'request':{'type':'status'}}).encode()
 with socket.socket(socket.AF_UNIX) as client:
  client.settimeout(3);client.connect(str(Path(os.environ['XDG_RUNTIME_DIR'])/'ottrin/index-v1.sock'));client.sendall(struct.pack('!I',len(body))+body)
  def exact(n):
   data=b''
   while len(data)<n:
    part=client.recv(n-len(data));assert part;data+=part
   return data
  n=struct.unpack('!I',exact(4))[0];assert n<1024*1024
  response=json.loads(exact(n));assert response['version']==2 and response['response']['type']=='status'
  return response['response']['diagnostics']
def ready():
 d=diagnostic();return d if d['status']=='Ready' and d['indexed_items']==1002 else None
def visible(text):return any(node=='label | '+text for node in q.visible_a11y_nodes())
def capture(name):
 ImageGrab.grab(xdisplay=os.environ['DISPLAY']).save(O/(name+'.png'))
 (O/(name+'.a11y.txt')).write_text('\n'.join(q.visible_a11y_nodes())+'\n')
def find_window(title):return q.wait_for(lambda:q.run('xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name',title,capture=True,check=False).stdout.strip(),'owned '+title)
def launch(name,probe_error=False):
 global app
 env=os.environ.copy()
 if probe_error:env['PATH']=str(B/'probe-bin')+':'+env['PATH']
 with (O/(name+'.log')).open('w') as log:app=subprocess.Popen([str(image),str(B/'fixtures')],env=env,stdout=log,stderr=log)
 q.target_process_id=app.pid;q.wait_for(q.ottrin_app,'owned application',timeout=30)
 q.wait_for(lambda:q.visible_entry('owned-0005.txt'),'main listing ready')
 actual=hashlib.sha256(Path(f'/proc/{app.pid}/exe').read_bytes()).hexdigest();assert actual==input_data['gui_sha256']
 q.click_accessible_name('Manage administrator helper','push button')
 settings=find_window('Ottrin Settings');assert len(settings.split())==1
 geometry=q.run('xdotool','getwindowgeometry','--shell',settings,capture=True).stdout
 fields=dict(line.split('=',1) for line in geometry.splitlines() if '=' in line)
 assert abs(int(fields['WIDTH'])-round(920*float(os.environ['WINIT_X11_SCALE_FACTOR'])))<=8
 x=int(fields['X'])+int(fields['WIDTH'])//2;y=int(fields['Y'])+int(fields['HEIGHT'])//2
 q.run('xdotool','mousemove',str(x),str(y));q.run('xdotool','click','--repeat','14','--delay','60','5');time.sleep(.5)
 return settings,actual,geometry
def close(settings):
 q.run('xdotool','windowactivate','--sync',settings);q.run('xdotool','key','--clearmodifiers','alt+F4')
 main=find_window('^Ottrin$');assert len(main.split())==1
 q.xkey(main,'alt+F4');assert app.wait(timeout=15)==0
try:
 assert os.environ.get('XDG_SESSION_ID')
 run('dbus-update-activation-environment','--systemd','DISPLAY','XDG_SESSION_TYPE','GTK_MODULES','NO_AT_BRIDGE')
 run('gsettings','set','org.gnome.desktop.interface','toolkit-accessibility','true')
 display=subprocess.Popen(['Xvfb',':99','-screen','0','1920x1200x24','-nolisten','tcp'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 os.environ['DISPLAY']=':99';q.wait_for(lambda:q.run('xdotool','getdisplaygeometry',capture=True,check=False).stdout.strip()=='1920 1200','owned display')
 with (O/'openbox.log').open('w') as log:wm=subprocess.Popen(['openbox'],stdout=log,stderr=subprocess.STDOUT)
 def wm_ready():
  assert wm.poll() is None,('Openbox exited',wm.returncode,(O/'openbox.log').read_text())
  return '_NET_SUPPORTING_WM_CHECK(WINDOW)' in q.run('xprop','-root','_NET_SUPPORTING_WM_CHECK',capture=True,check=False).stdout
 q.wait_for(wm_ready,'owned window manager readiness',timeout=15)
 input_data=json.loads((B/'APPIMAGE_INPUT.json').read_text());image=B/input_data['name']
 assert hashlib.sha256(image.read_bytes()).hexdigest()==input_data['sha256']
 report['install']=run('/usr/bin/ottrin','index','service-install');assert enabled()
 report['initial_index']=q.wait_for(ready,'installed service Ready before GUI',timeout=60)
 before=unit();assert before['ActiveState']=='active' and int(before['MainPID'])>0
 daemon_sha=hashlib.sha256((Path('/proc')/before['MainPID']/'exe').read_bytes()).hexdigest();assert daemon_sha==input_data['daemon_sha256'];report['daemon_sha256']=daemon_sha
 database=B/'.cache/ottrin/search.db';db_identity=(database.stat().st_dev,database.stat().st_ino)
 probe=B/'probe-bin';probe.mkdir();script=probe/'systemctl'
 script.write_text('#!/bin/sh\nif [ "$1" = "--user" ] && [ "$2" = "is-enabled" ] && [ "$3" = "ottrin-index.service" ]; then\n  echo "Owned QA automatic startup probe unavailable" >&2\n  exit 1\nfi\nexec /usr/bin/systemctl "$@"\n');script.chmod(0o700)
 for scale in [1.0,1.5]:
  os.environ['WINIT_X11_SCALE_FACTOR']=str(scale)
  for theme in ['Dark','Light']:
   pref=B/'.config/ottrin/config.json';config=json.loads(pref.read_text());config.update(theme=theme,default_view_mode='Miller',window_size=[800,500]);pref.write_text(json.dumps(config))
   assert enabled();name=f'startup-{theme.lower()}-{scale}'
   settings,actual,geometry=launch(name)
   q.wait_for(lambda:visible('Automatic startup at sign-in: On'),'actual On label');capture(name+'-on')
   q.click_accessible_name('Start index automatically at sign-in','check box')
   q.wait_for(lambda:not enabled(),'real registration disabled');q.wait_for(lambda:visible('Automatic startup at sign-in: Off'),'actual Off label');capture(name+'-off')
   assert unit()['MainPID']==before['MainPID'] and ready()
   q.click_accessible_name('Start index automatically at sign-in','check box')
   q.wait_for(enabled,'real registration enabled');q.wait_for(lambda:visible('Automatic startup at sign-in: On'),'On after round trip')
   assert unit()['MainPID']==before['MainPID'];close(settings);assert ready()
   settings,actual,geometry=launch(name+'-error',True)
   q.wait_for(lambda:visible('Automatic startup at sign-in: Unavailable'),'unavailable startup label')
   assert any('Owned QA automatic startup probe unavailable' in node for node in q.visible_a11y_nodes())
   assert not any('Start index automatically at sign-in' in node for node in q.visible_a11y_nodes()),'Unknown startup must not offer an unchecked editable toggle'
   q.wait_for(lambda:visible('Running — index ready'),'running index stays Ready during startup-probe failure')
   assert enabled() and unit()['MainPID']==before['MainPID']
   capture(name+'-unavailable');close(settings)
   report['cases'].append({'theme':theme,'backend_scale':scale,'gui_sha256':actual,'settings_geometry':geometry,'registration_round_trip':'On -> Off -> On through real checkbox and systemctl inspection','running_pid_preserved':before['MainPID'],'error_scope':'Owned systemctl wrapper fails is-enabled only; all other commands use /usr/bin/systemctl','captures':[name+'-on.png',name+'-off.png',name+'-unavailable.png'],'normal_settings_and_main_close':True})
 assert ready() and enabled() and unit()['MainPID']==before['MainPID']
 assert (database.stat().st_dev,database.stat().st_ino)==db_identity
 assert json.loads((B/'.config/ottrin/config.json').read_text())['search']['include_roots']==[str(B/'fixtures')]
 assert all((B/'fixtures'/f'owned-{i:04}.txt').read_text()==f'owned{i}\n' for i in range(1000))
 report.update(status='pass',database_identity_preserved=True,saved_roots_preserved=True,sources_preserved=True,daemon_running_after_normal_gui_close=True)
except BaseException as error:
 report.update(status='fail',error=repr(error),stdout=getattr(error,'stdout',None),stderr=getattr(error,'stderr',None),traceback=traceback.format_exc())
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
 (O/'WORKER_RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
while True:signal.pause()
