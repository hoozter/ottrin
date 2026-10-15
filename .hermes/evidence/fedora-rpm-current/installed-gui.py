import os,pathlib,subprocess,time,json,hashlib,signal
import getpass,socket
if getpass.getuser() != 'ottrinqa' or socket.gethostname() != 'ottrin-fedora-rpm-qa':
 raise RuntimeError('This destructive package lifecycle fixture is restricted to the owned Fedora QA guest')
home=pathlib.Path.home();out=home/'qa-current';out.mkdir(exist_ok=True);fixture=home/'OttrinFixtures';fixture.mkdir(exist_ok=True);source=fixture/'rpm-native-fixture.txt';source.write_text('Fedora native RPM preview fixture\n');original=source.read_bytes();config=home/'.config/ottrin/config.json';config.parent.mkdir(parents=True,exist_ok=True);assert not config.exists(),'Expected fresh QA config'
config.write_text(json.dumps({'default_view_mode':'Miller','search':{'include_roots':[str(fixture)],'content_indexing_enabled':False},'search_setup_completed':True}))
result={'status':'running','scope':'Fedora 44 Cloud, installed RPM, ordinary user, private Xvfb and software OpenGL'};app=None;display=None;env=os.environ.copy()
def run(args,timeout=30,check=True):
 p=subprocess.run(args,timeout=timeout,capture_output=True,text=True,env=env)
 if check and p.returncode:raise RuntimeError(f'{args}: exit {p.returncode}: {p.stderr[-2000:]} {p.stdout[-1000:]}')
 return p

def cli(*args):return run(['/usr/bin/ottrin',*args]).stdout

def pid():return int(run(['systemctl','--user','show','ottrin-index.service','--property=MainPID','--value']).stdout.strip())
def query(term):return json.loads(cli('search',term,'--json'))
def interrupted(signum,frame):raise RuntimeError(f'Interrupted {signum}')
for sig in [signal.SIGTERM,signal.SIGINT]:signal.signal(sig,interrupted)
try:
 result['service_install']=cli('index','service-install');before=pid();assert before>0;result['before_gui_pid']=before
 result['index_update']=json.loads(cli('index','update','--json'));assert query('rpm-native-fixture')['total']==1
 r,w=os.pipe();display=subprocess.Popen(['Xvfb','-displayfd',str(w),'-screen','0','1200x900x24','-nolisten','tcp'],pass_fds=(w,),stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL);os.close(w);number=os.read(r,64).decode().strip();os.close(r);assert number
 env.update(DISPLAY=':'+number,OTTRIN_RENDERER='opengl',LIBGL_ALWAYS_SOFTWARE='1',WINIT_UNIX_BACKEND='x11',WINIT_X11_SCALE_FACTOR='1',OTTRIN_DEV_HELPER_AUTOBUILD='0');env.pop('WAYLAND_DISPLAY',None)
 with (out/'app.log').open('w') as log:
  app=subprocess.Popen(['/usr/bin/ottrin','--select',str(source)],env=env,stdout=log,stderr=subprocess.STDOUT)
  deadline=time.monotonic()+30;win=None
  while time.monotonic()<deadline:
   assert app.poll() is None,'GUI exited'
   p=run(['xdotool','search','--onlyvisible','--name','Ottrin'],check=False)
   if p.returncode==0 and p.stdout.strip():win=p.stdout.splitlines()[0];break
   time.sleep(.2)
  assert win,'GUI window timeout';time.sleep(3)
  run(['import','-window',win,str(out/'installed-rpm-main.png')]);assert pid()==before,'GUI replaced configured daemon'
  dims=dict(line.split('=',1) for line in run(['xdotool','getwindowgeometry','--shell',win]).stdout.splitlines() if '=' in line)
  run(['xdotool','mousemove','--window',win,str(int(dims['WIDTH'])-22),'18','click','1']);assert app.wait(timeout=15)==0;result['gui_exit']=0;app=None
 assert pid()==before,'GUI close stopped/replaced daemon'
 after=fixture/'rpm-after-window-close.txt';after.write_text('Detected after window closed\n');started=time.monotonic();deadline=started+15
 while time.monotonic()<deadline:
  if query('rpm-after-window-close')['total']==1:break
  time.sleep(.15)
 else:raise RuntimeError('After-close watcher failed')
 result['after_close_watch_ms']=round((time.monotonic()-started)*1000);assert pid()==before
 pref=config.read_bytes();result['preference_sha256']=hashlib.sha256(pref).hexdigest()
 reinstall=run(['sudo','dnf','-y','reinstall','./ottrin.rpm'],timeout=180);(out/'reinstall.log').write_text(reinstall.stdout+reinstall.stderr);assert config.read_bytes()==pref;assert source.read_bytes()==original;run(['rpm','-V','ottrin']);assert pid()==before;result['same_version_reinstall']='pass; payload verification, preferences/source and running daemon preserved'
 result['restart']=cli('index','service-restart');restarted=pid();assert restarted>0 and restarted!=before;result['restarted_pid']=restarted;assert query('rpm-native-fixture')['total']==1
 result['service_remove']=cli('index','service-remove');assert not (home/'.config/systemd/user/ottrin-index.service').exists();assert config.read_bytes()==pref
 remove=run(['sudo','dnf','-y','remove','ottrin'],timeout=180);(out/'uninstall.log').write_text(remove.stdout+remove.stderr)
 assert run(['rpm','-q','ottrin'],check=False).returncode!=0;assert not pathlib.Path('/usr/bin/ottrin').exists();assert config.read_bytes()==pref;assert source.read_bytes()==original
 result['uninstall']='pass after explicit user service removal; preferences and source preserved';result['status']='pass'
except Exception as e:result['status']='fail';result['error']=str(e)
finally:
 if app and app.poll() is None:
  app.terminate()
  try:app.wait(timeout=5)
  except subprocess.TimeoutExpired:app.kill();app.wait()
 if display:
  display.terminate()
  try:display.wait(timeout=5)
  except subprocess.TimeoutExpired:display.kill();display.wait()
 cleanup=run(['systemctl','--user','stop','ottrin-index.service'],check=False);result['cleanup_service_stop_exit']=cleanup.returncode
 result['test_display_stopped']=display is None or display.poll() is not None
 (out/'result.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2));raise SystemExit(0 if result['status']=='pass' else 1)
