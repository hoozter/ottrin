"""Actual mounted package row/thumbnail/footer/Ready-color review; owned display only."""
from pathlib import Path
import hashlib,importlib.util,json,os,signal,socket,struct,subprocess,time,traceback
from PIL import ImageGrab
B=Path.home();O=B/'results';app=None;wm=None;display=None
report={'status':'running','cases':[],'footer_samples':[],'services':[]}
spec=importlib.util.spec_from_file_location('qa',B/'linux-desktop-qa.py');q=importlib.util.module_from_spec(spec);spec.loader.exec_module(q)
def run(*args):
 p=subprocess.run(args,capture_output=True,text=True,timeout=45);assert p.returncode==0,(args,p.returncode,p.stdout,p.stderr);return p.stdout
def diagnostic():
 body=json.dumps({'version':2,'request':{'type':'status'}}).encode()
 with socket.socket(socket.AF_UNIX) as client:
  client.settimeout(3);client.connect(str(Path(os.environ['XDG_RUNTIME_DIR'])/'ottrin/index-v1.sock'));client.sendall(struct.pack('!I',len(body))+body)
  def exact(n):
   value=b''
   while len(value)<n:
    part=client.recv(n-len(value));assert part;value+=part
   return value
  n=struct.unpack('!I',exact(4))[0];assert n<1024*1024
  return json.loads(exact(n))['response']['diagnostics']
def ready():
 try:
  value=diagnostic();return value if value['status']=='Ready' and value['indexed_items']==5 else None
 except (OSError,KeyError):return None
def visible(text):return any(n=='label | '+text for n in q.visible_a11y_nodes())
def capture(name):
 ImageGrab.grab(xdisplay=os.environ['DISPLAY']).save(O/(name+'.png'))
 (O/(name+'.a11y.txt')).write_text('\n'.join(q.visible_a11y_nodes())+'\n')
def window(title):
 value=q.wait_for(lambda:q.run('xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name',title,capture=True,check=False).stdout.strip(),'owned '+title)
 assert len(value.split())==1;return value
def launch(label,theme,mode,density,backend=1.0,thumbnails=False):
 global app
 os.environ['WINIT_X11_SCALE_FACTOR']=str(backend)
 pref=B/'.config/ottrin/config.json';config=json.loads(pref.read_text());config.update(theme=theme,theme_preset='Ottrin',active_custom_theme=None,custom_themes=[],default_view_mode=mode,window_size=[900,600],list_show_thumbnails=thumbnails,miller_view_scale=1.0,list_view_scale=1.0,grid_view_scale=1.0)
 config['miller_view_scale' if mode=='Miller' else 'list_view_scale']=density
 pref.write_text(json.dumps(config))
 with (O/(label+'.log')).open('w') as log:app=subprocess.Popen([str(image),str(B/'fixtures')],stdout=log,stderr=log)
 q.target_process_id=app.pid;q.wait_for(q.ottrin_app,'owned mapped app',timeout=30)
 main=window('^Ottrin$');q.wait_for(lambda:q.visible_entry(wide),'actual wide filename');q.wait_for(lambda:visible('4 items'),'actual four visible entries, not five indexed')
 actual=hashlib.sha256(Path(f'/proc/{app.pid}/exe').read_bytes()).hexdigest();assert actual==input_data['gui_sha256']
 q.run('xdotool','mousemove','5','5');return main,actual
def close(main):
 q.xkey(main,'alt+F4');assert app.wait(timeout=15)==0
try:
 assert os.environ.get('XDG_SESSION_ID')
 run('dbus-update-activation-environment','--systemd','DISPLAY','XDG_SESSION_TYPE','GTK_MODULES','NO_AT_BRIDGE')
 run('gsettings','set','org.gnome.desktop.interface','toolkit-accessibility','true')
 display=subprocess.Popen(['Xvfb',':99','-screen','0','1920x1200x24','-nolisten','tcp'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 os.environ['DISPLAY']=':99';q.wait_for(lambda:q.run('xdotool','getdisplaygeometry',capture=True,check=False).stdout.strip()=='1920 1200','owned display')
 wm=subprocess.Popen(['openbox'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 def wm_ready():
  assert wm.poll() is None
  return '_NET_SUPPORTING_WM_CHECK(WINDOW)' in q.run('xprop','-root','_NET_SUPPORTING_WM_CHECK',capture=True,check=False).stdout
 q.wait_for(wm_ready,'owned EWMH manager',timeout=15)
 input_data=json.loads((B/'APPIMAGE_INPUT.json').read_text());image=B/input_data['name'];assert hashlib.sha256(image.read_bytes()).hexdigest()==input_data['sha256']
 wide='00-WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW.txt'
 report['service_install']=run('/usr/bin/ottrin','index','service-install');assert run('systemctl','--user','is-enabled','ottrin-index.service').strip()=='enabled'
 report['initial_index']=q.wait_for(ready,'Ready5 indexed entries',timeout=60)
 before=run('systemctl','--user','show','ottrin-index.service','--property=MainPID','--value').strip();assert int(before)>0
 assert hashlib.sha256(Path('/proc',before,'exe').read_bytes()).hexdigest()==input_data['daemon_sha256']
 database=B/'.cache/ottrin/search.db';identity=(database.stat().st_dev,database.stat().st_ino)
 for theme in ['Dark','Light']:
  for mode in ['Miller','List']:
   for density,backend in [(.75,1.0),(1.85,1.0),(1.85,1.5)]:
    label=f'{mode.lower()}-{theme.lower()}-{density}-{backend}';main,actual=launch(label,theme,mode,density,backend)
    row=q.visible_entry(wide);bounds=row.queryComponent().getExtents(q.pyatspi.DESKTOP_COORDS)
    time.sleep(.7);capture(label)
    report['cases'].append({'theme':theme,'mode':mode,'density':density,'backend_scale':backend,'gui_sha256':actual,'capture':label+'.png','row_height':bounds.height})
    if not report['footer_samples']:
     start=time.monotonic()
     for seconds in [0,2,10]:
      delay=start+seconds-time.monotonic()
      if delay>0:time.sleep(delay)
      assert visible('4 items');report['footer_samples'].append({'seconds':time.monotonic()-start,'visible_items':4,'index_total':diagnostic()['indexed_items']})
    close(main)
  label='thumbnails-'+theme.lower();main,actual=launch(label,theme,'List',1.85,1.0,True)
  q.wait_for(lambda:q.visible_entry('02-preview.png'),'actual image file row');time.sleep(3);capture(label)
  report['cases'].append({'theme':theme,'mode':'List','density':1.85,'backend_scale':1.0,'thumbnails':True,'gui_sha256':actual,'capture':label+'.png'})
  close(main)
  label='services-'+theme.lower();main,actual=launch(label,theme,'Miller',1.0)
  q.click_accessible_name('Manage administrator helper','push button');settings=window('Ottrin Settings')
  q.run('xdotool','windowsize','--sync',settings,'920','900')
  q.wait_for(lambda:visible('Installed and ready'),'actual helper Ready text');q.wait_for(lambda:visible('Running — index ready'),'actual index Ready text')
  time.sleep(.7);capture(label);report['services'].append({'theme':theme,'gui_sha256':actual,'capture':label+'.png'})
  q.xkey(settings,'alt+F4');close(main)
 assert ready() and run('systemctl','--user','show','ottrin-index.service','--property=MainPID','--value').strip()==before
 assert (database.stat().st_dev,database.stat().st_ino)==identity
 report.update(status='pass',database_identity_preserved=True,running_pid_preserved=before,daemon_ready_after_normal_close=True,scope='Actual AppImage/FUSE in owned software-X11 guest; changed visual behavior only. Original captures require direct review; no full interface/platform acceptance.')
except BaseException as error:
 report.update(status='fail',error=repr(error),traceback=traceback.format_exc())
 try:capture('failure')
 except Exception:pass
finally:
 for process in [app,wm,display]:
  if process is not None and process.poll() is None:
   process.terminate()
   try:process.wait(timeout=10)
   except subprocess.TimeoutExpired:process.kill();process.wait()
 (O/'WORKER_RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
while True:signal.pause()
