"""Real desktop-login startup and normal GUI-close acceptance; no SSH login as this user."""
from pathlib import Path
import hashlib,importlib.util,json,os,signal,socket,struct,subprocess,time,traceback
from PIL import ImageGrab
base=Path.home();out=base/'results';state=base/'login-stage.json'
phase=json.loads(state.read_text())['next'] if state.exists() else 0
boot=Path('/proc/sys/kernel/random/boot_id').read_text().strip()
report={'status':'running','phase':phase,'boot_id':boot,'session_id':os.environ.get('XDG_SESSION_ID'),'gui_attempts':0}
spec=importlib.util.spec_from_file_location('qa',base/'linux-desktop-qa.py');q=importlib.util.module_from_spec(spec);spec.loader.exec_module(q)
app=None

def run(*args):
 p=subprocess.run(args,capture_output=True,text=True,timeout=45)
 if p.returncode:raise RuntimeError(f'{args}: exit {p.returncode}; stdout={p.stdout!r}; stderr={p.stderr!r}')
 return p
def unit():return dict(line.split('=',1) for line in run('systemctl','--user','show','ottrin-index.service','-p','MainPID','-p','ActiveState').stdout.splitlines())
def enabled():
 p=subprocess.run(['systemctl','--user','is-enabled','ottrin-index.service'],capture_output=True,text=True,timeout=10)
 assert p.returncode in (0,1) and p.stdout.strip() in {'enabled','disabled'},(p.returncode,p.stdout,p.stderr)
 return p.stdout.strip()=='enabled'
def wait(fn,label,seconds=60):
 deadline=time.monotonic()+seconds
 while time.monotonic()<deadline:
  try:
   value=fn()
   if value:return value
  except OSError:pass
  time.sleep(.1)
 raise AssertionError(label)
def status():
 data=json.dumps({'version':2,'request':{'type':'status'}}).encode()
 with socket.socket(socket.AF_UNIX) as client:
  client.settimeout(3);client.connect(str(Path(os.environ['XDG_RUNTIME_DIR'])/'ottrin/index-v1.sock'));client.sendall(struct.pack('!I',len(data))+data)
  def exact(n):
   value=b''
   while len(value)<n:
    part=client.recv(n-len(value));assert part;value+=part
   return value
  count=struct.unpack('!I',exact(4))[0];assert count<1024*1024
  reply=json.loads(exact(count));assert reply['version']==2 and reply['response']['type']=='status'
  return reply['response']['diagnostics']
def ready():
 d=status();return d if d['status']=='Ready' and d['watcher_ready'] and d['indexed_items']>=1000 else None
def window(name):
 return q.wait_for(lambda:q.run('xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name',name,capture=True,check=False).stdout.strip(),'owned window '+name)
def capture(name):ImageGrab.grab(xdisplay=os.environ['DISPLAY']).save(out/(name+'.png'))
try:
 if phase:assert json.loads(state.read_text())['last_boot_id']!=boot,'A new login alone must not count as a reboot'
 assert report['session_id']
 report['session']=run('loginctl','show-session',report['session_id'],'-p','Active','-p','Remote','-p','Seat','-p','Type','-p','Service').stdout
 assert 'Active=yes\n' in report['session'] and 'Seat=seat0\n' in report['session'] and 'Service=lightdm-autologin\n' in report['session']
 run('dbus-update-activation-environment','--systemd','DISPLAY','XDG_SESSION_TYPE','XDG_CURRENT_DESKTOP','GTK_MODULES','NO_AT_BRIDGE')
 run('gsettings','set','org.gnome.desktop.interface','toolkit-accessibility','true')
 wm=subprocess.Popen(['openbox'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 if phase==0:
  report['install']=run('/usr/bin/ottrin','index','service-install').stdout
  assert enabled()
 elif phase==1:
  assert enabled(),'Enabled startup was lost across the reboot'
  before=wait(lambda:unit() if int(unit()['MainPID'])>0 else None,'Service did not start at desktop login before GUI launch')
  report['before_gui_auto_started_unit']=before
 else:
  assert phase==2 and not enabled(),'Disabled startup was not preserved'
  for _ in range(30):
   assert unit()['MainPID']=='0','Disabled service unexpectedly started before opening Ottrin'
   assert not (Path(os.environ['XDG_RUNTIME_DIR'])/'ottrin/index-v1.sock').exists()
   time.sleep(.1)
  report['disabled_before_gui_no_daemon']=True
 if phase<2:
  report['before_gui_ready']=wait(ready,'Pre-GUI managed index readiness')
  before=unit();report['before_gui_unit']=before
 with (out/('app-phase'+str(phase)+'.log')).open('w') as log:
  app=subprocess.Popen(['/usr/bin/ottrin',str(base/'fixtures')],stdout=log,stderr=log);report['gui_attempts']+=1;q.target_process_id=app.pid
  q.wait_for(q.ottrin_app,'first and only GUI launch',timeout=30)
  report['after_gui_ready']=wait(ready,'GUI could not connect to the background index')
  running=unit();report['after_gui_unit']=running
  assert int(running['MainPID'])>0
  if phase<2:assert running['MainPID']==before['MainPID'],'Opening GUI replaced an already running managed daemon'
  if phase==1:
   q.click_accessible_name('Manage administrator helper','push button')
   settings=window('^Ottrin Settings$');assert len(settings.split())==1
   q.run('xdotool','mousemove','--window',settings,'500','500');q.run('xdotool','click','--repeat','6','--delay','50','5')
   q.click_accessible_name('Start index automatically at sign-in')
   wait(lambda:not enabled(),'Native checkbox did not disable login startup',30)
   assert unit()['MainPID']==running['MainPID'],'Login checkbox changed current daemon lifetime'
   capture('disabled-login-running-index');q.xkey(settings,'alt+F4')
  main=window('^Ottrin$');assert len(main.split())==1
  if phase==2:
   start=time.monotonic();samples=[]
   for seconds in [0,2,10,30]:
    remaining=start+seconds-time.monotonic()
    if remaining>0:time.sleep(remaining)
    nodes=q.visible_a11y_nodes();(out/f'warm-disabled-{seconds}.a11y.txt').write_text('\n'.join(nodes)+'\n');capture(f'warm-disabled-{seconds}')
    samples.append({'seconds_after_window':time.monotonic()-start,'item_labels':[line for line in nodes if ' items' in line]})
   report['no_input_startup_samples']=samples
  capture('main-phase'+str(phase));q.xkey(main,'alt+F4');assert app.wait(timeout=15)==0
  for _ in range(20):
   assert unit()['MainPID']==running['MainPID'],'Closing the last GUI stopped or replaced the managed daemon'
   assert ready();time.sleep(.1)
  report['after_normal_gui_close_unit']=unit()
 pref=json.loads((base/'.config/ottrin/config.json').read_text())
 assert pref['default_view_mode']=='Miller' and pref['theme']=='Dark'
 assert pref['search']['include_roots']==[str(base/'fixtures')] and not pref['search']['content_indexing_enabled']
 assert all((base/'fixtures'/f'owned-{i:04}.txt').read_text()==f'owned{i}\n' for i in range(1000))
 store=base/'.cache/ottrin/search.db';report['database_inode']=store.stat().st_ino
 if phase:assert report['database_inode']==json.loads((out/'PHASE0.json').read_text())['database_inode']
 report.update(status='pass',source_and_preferences_preserved=True,startup_enabled=enabled())
 state.write_text(json.dumps({'next':phase+1,'last_boot_id':boot}))
except BaseException as error:
 report.update(status='fail',error=repr(error),traceback=traceback.format_exc())
 try:capture('failure-phase'+str(phase))
 except Exception:pass
finally:
 if app is not None and app.poll() is None:
  app.terminate()
  try:app.wait(timeout=10)
  except subprocess.TimeoutExpired:app.kill();app.wait()
 temporary=out/('PHASE'+str(phase)+'.next');temporary.write_text(json.dumps(report,indent=2)+'\n');temporary.replace(out/('PHASE'+str(phase)+'.json'))
# Keep this actual desktop login alive until the controller reboots or tears down
# its owned VM. Returning would allow LightDM to start a second test login.
while True:signal.pause()
