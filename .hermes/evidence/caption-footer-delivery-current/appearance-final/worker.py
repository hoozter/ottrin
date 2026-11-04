"""No-input settled-count observation and exact-source Grid visual capture."""
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
 with (out/'openbox.log').open('w') as wm_log:
  wm=subprocess.Popen(['openbox'],stdout=wm_log,stderr=subprocess.STDOUT)
 def wm_ready():
  assert wm.poll() is None,('Owned Openbox exited',wm.returncode,(out/'openbox.log').read_text())
  return '_NET_SUPPORTING_WM_CHECK(WINDOW)' in q.run('xprop','-root','_NET_SUPPORTING_WM_CHECK',capture=True,check=False).stdout
 q.wait_for(wm_ready,'owned window manager EWMH ready before launching GUI',timeout=15)
 report['window_manager_ready']=q.run('xprop','-root','_NET_SUPPORTING_WM_CHECK',capture=True).stdout
 image=base/json.loads((base/'APPIMAGE_INPUT.json').read_text())['name']
 for scale in [1.0,1.5]:
  os.environ['WINIT_X11_SCALE_FACTOR']=str(scale)
  for theme in ['Dark','Light']:
   pref=base/'.config/ottrin/config.json';config=json.loads(pref.read_text());config.update(theme=theme,default_view_mode='Miller',window_size=[800,500]);pref.write_text(json.dumps(config))
   label=f'appearance-scrolled-{theme.lower()}-{scale}';window=launch(str(image),label)
   q.wait_for(lambda:q.visible_entry('owned-0005.txt'),'main listing ready before opening Settings')
   q.click_accessible_name('Manage administrator helper','push button');q.click_accessible_name('Appearance')
   settings=q.wait_for(lambda:q.run('xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','Ottrin Settings',capture=True,check=False).stdout.strip(),'Settings window')
   assert len(settings.split())==1
   geometry=q.run('xdotool','getwindowgeometry','--shell',settings,capture=True).stdout
   fields=dict(line.split('=',1) for line in geometry.splitlines() if '=' in line)
   x=int(fields['X'])+int(fields['WIDTH'])//2;y=int(fields['Y'])+int(fields['HEIGHT'])//2
   q.run('xdotool','mousemove',str(x),str(y));q.run('xdotool','click','--repeat','10','--delay','70','5');time.sleep(1)
   capture(label);(out/(label+'.a11y.txt')).write_text('\n'.join(q.visible_a11y_nodes())+'\n')
   actual=hashlib.sha256(Path(f'/proc/{app.pid}/exe').read_bytes()).hexdigest();assert actual==json.loads((base/'APPIMAGE_INPUT.json').read_text())['gui_sha256']
   report['appearance_cases'].append({'theme':theme,'backend_scale':scale,'gui_sha256':actual,'capture':label+'.png','settings_geometry':geometry})
   # Close the actual Settings viewport before addressing the current main window.
   q.run('xdotool','windowactivate','--sync',settings,capture=True);q.run('xdotool','key','--clearmodifiers','alt+F4')
   main=q.wait_for(lambda:q.run('xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','^Ottrin$',capture=True,check=False).stdout.strip(),'main remains open after Settings close')
   assert len(main.split())==1
   q.xkey(main,'alt+F4');assert app.wait(timeout=15)==0
 assert all((base/'fixtures'/f'owned-{i:04}.txt').read_text()==f'owned{i}\n' for i in range(1000))
 report['status']='pass'
 report['cold_scope']='Fresh account with installed/enabled daemon confirmed Ready before first GUI; no input during footer samples. No raw systemctl-stop precondition.'
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
 (out/'WORKER_RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
while True:signal.pause()
