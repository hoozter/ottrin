"""Real stable-path AppImage/portable replacement with a running user index."""
from pathlib import Path
import hashlib,importlib.util,json,os,shutil,socket,struct,subprocess,time,traceback
from PIL import ImageGrab
base=Path.home();out=base/'results';config=json.loads((base/'REPLACEMENT_INPUT.json').read_text());report={'status':'running','cases':[]};app=None
spec=importlib.util.spec_from_file_location('qa',base/'linux-desktop-qa.py');q=importlib.util.module_from_spec(spec);spec.loader.exec_module(q)
def run(*args,check=True):
 p=subprocess.run(list(map(str,args)),capture_output=True,text=True,timeout=60)
 if check and p.returncode:raise RuntimeError(f'{args}: {p.returncode}; {p.stdout}; {p.stderr}')
 return p
def wait(fn,label,seconds=45):
 deadline=time.monotonic()+seconds
 while time.monotonic()<deadline:
  try:
   value=fn()
   if value:return value
  except OSError:pass
  time.sleep(.1)
 raise AssertionError(label)
def unit():return dict(line.split('=',1) for line in run('systemctl','--user','show','ottrin-index.service','-p','MainPID','-p','ActiveState').stdout.splitlines())
def enabled():
 p=run('systemctl','--user','is-enabled','ottrin-index.service',check=False)
 assert p.returncode in (0,1) and p.stdout.strip() in {'enabled','disabled'},(p.stdout,p.stderr)
 return p.stdout.strip()=='enabled'
def sha(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()
def ready():
 with socket.socket(socket.AF_UNIX) as client:
  client.settimeout(3);client.connect(str(Path(os.environ['XDG_RUNTIME_DIR'])/'ottrin/index-v1.sock'))
  data=json.dumps({'version':2,'request':{'type':'status'}}).encode();client.sendall(struct.pack('!I',len(data))+data)
  def exact(n):
   value=b''
   while len(value)<n:
    part=client.recv(n-len(value));assert part;value+=part
   return value
  size=struct.unpack('!I',exact(4))[0];assert size<=1024*1024
  frame=json.loads(exact(size));assert frame['version']==2 and frame['response']['type']=='status'
  d=frame['response']['diagnostics'];return d if d['status']=='Ready' and d['watcher_ready'] and d['indexed_items']>=100 else None
def no_consumers(path):
 files=[p for p in path.rglob('*') if p.is_file() and not p.is_symlink()] if path.is_dir() else [path]
 for start in range(0,len(files),40):
  p=run('fuser',*files[start:start+40],check=False);assert p.returncode in (0,1),p.stderr
  if p.returncode==0:return False
 return True
def remove(path):
 if not path.exists():return
 wait(lambda:no_consumers(path),'Retired package still has a consumer',15)
 if path.is_dir():shutil.rmtree(path)
 else:path.unlink()
def gui(executable,label,expected):
 global app
 with (out/(label+'.log')).open('w') as log:
  app=subprocess.Popen([str(executable),str(base/'fixtures')],stdout=log,stderr=log);q.target_process_id=app.pid
  q.wait_for(q.ottrin_app,'single '+label+' GUI launch',timeout=30)
  assert sha(f'/proc/{app.pid}/exe')==expected['gui_sha256']
  d=wait(ready,label+' index readiness');state=unit();pid=state['MainPID'];assert int(pid)>0
  assert sha(f'/proc/{pid}/exe')==expected['daemon_sha256']
  main=q.wait_for(lambda:q.run('xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','^Ottrin$',capture=True,check=False).stdout.strip(),'owned main window')
  assert len(main.split())==1
  time.sleep(.5);ImageGrab.grab(xdisplay=os.environ['DISPLAY']).save(out/(label+'.png'))
  q.xkey(main,'alt+F4');assert app.wait(timeout=15)==0
  assert unit()['MainPID']==pid and ready()
  pref=json.loads((base/'.config/ottrin/config.json').read_text());assert pref['default_view_mode']=='Grid' and pref['theme']=='Dark'
  assert pref['search']['include_roots']==[str(base/'fixtures')]
  return {'pid':pid,'diagnostics':d,'startup_enabled':enabled(),'gui_attempts':1,'view_preference':'Grid','daemon_sha256':expected['daemon_sha256']}
try:
 run('dbus-update-activation-environment','--systemd','DISPLAY','XDG_SESSION_TYPE','XDG_CURRENT_DESKTOP','GTK_MODULES','NO_AT_BRIDGE')
 run('gsettings','set','org.gnome.desktop.interface','toolkit-accessibility','true')
 wm=subprocess.Popen(['openbox'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 for kind in ['portable','appimage']:
  for startup in [True,False]:
   case=kind+('-enabled' if startup else '-disabled');assets=base/'assets'
   live=base/('live-portable' if kind=='portable' else 'live.AppImage');next_path=base/('next-portable' if kind=='portable' else 'next.AppImage');retired=base/('retired-'+case)
   for path in [live,next_path,retired]:remove(path)
   if kind=='portable':
    shutil.copytree(assets/'old-portable',live);shutil.copytree(assets/'new-portable',next_path)
    executable=live/config['portable_root']/'bin/ottrin'
   else:
    shutil.copyfile(assets/'old.AppImage',live);shutil.copyfile(assets/'new.AppImage',next_path);live.chmod(0o755);next_path.chmod(0o755);executable=live
   run(executable,'index','service-install');wait(ready,'old installed service readiness')
   if not startup:run('systemctl','--user','disable','ottrin-index.service')
   assert enabled()==startup
   before=gui(executable,case+'-old',config['old']);assert before['startup_enabled']==startup
   inode=(base/'.cache/ottrin/search.db').stat().st_ino
   live.rename(retired);next_path.rename(live)
   after=gui(executable,case+'-new',config['new']);assert after['startup_enabled']==startup
   assert before['pid']!=after['pid'] and not Path('/proc',before['pid']).exists()
   assert (base/'.cache/ottrin/search.db').stat().st_ino==inode
   assert all((base/'fixtures'/f'owned-{i:04}.txt').read_text()==f'owned{i}\n' for i in range(100))
   run(executable,'index','service-remove')
   wait(lambda:not Path('/proc',after['pid']).exists(),'Service removal left the daemon running')
   assert not (base/'.config/systemd/user/ottrin-index.service').exists()
   remove(retired);remove(live)
   report['cases'].append({'case':case,'before':before,'after':after,'database_inode_preserved':True,'sources_preserved':True,'retired_payload_removed':True})
 report['status']='pass'
except BaseException as error:
 report.update(status='fail',error=repr(error),traceback=traceback.format_exc())
 try:ImageGrab.grab(xdisplay=os.environ['DISPLAY']).save(out/'failure.png')
 except Exception:pass
finally:
 if app is not None and app.poll() is None:
  app.terminate()
  try:app.wait(timeout=10)
  except subprocess.TimeoutExpired:app.kill();app.wait()
 (out/'WORKER_RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report,indent=2));raise SystemExit(0 if report['status']=='pass' else 1)
