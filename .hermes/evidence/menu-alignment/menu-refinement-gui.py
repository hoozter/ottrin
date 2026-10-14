import os,pathlib,subprocess,tempfile,time,signal,json,sys,shutil
out=pathlib.Path(sys.argv[2]).resolve();out.mkdir(parents=True,exist_ok=True)
operations=[]
binary=pathlib.Path(sys.argv[1]).resolve()
root=pathlib.Path(tempfile.mkdtemp(prefix='ottrin-menu-refinement-'))
app=None;display=None
def run(*args, **kw): return subprocess.run(*args, timeout=20, **kw)
def cancelled(signum, frame): raise SystemExit('QA interrupted by signal '+str(signum))
for sig in [signal.SIGINT, signal.SIGTERM]: signal.signal(sig, cancelled)
def private_pids():
 result=[]
 for p in pathlib.Path('/proc').iterdir():
  if not p.name.isdigit() or int(p.name)==os.getpid():continue
  try:
   e=dict(v.split(b'=',1) for v in (p/'environ').read_bytes().split(b'\0') if b'=' in v)
   if p.stat().st_uid==os.getuid() and e.get(b'HOME')==os.fsencode(root) and e.get(b'XDG_STATE_HOME')==os.fsencode(root/'state'):result.append(int(p.name))
  except OSError:pass
 return result
try:
 r,w=os.pipe();display=subprocess.Popen(['Xvfb','-displayfd',str(w),'-screen','0','1700x1200x24','-nolisten','tcp'],pass_fds=(w,),stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL);os.close(w)
 data=b''
 while not data.endswith(b'\n'):
  chunk=os.read(r,64)
  if not chunk:raise RuntimeError('Xvfb closed before complete display number')
  data+=chunk
 number=data.decode().strip();os.close(r)
 (root/'runtime').mkdir(mode=0o700)
 env=os.environ.copy();env.pop('WAYLAND_DISPLAY',None);env.update(XDG_RUNTIME_DIR=str(root/'runtime'),DISPLAY=':'+number,HOME=str(root),XDG_CONFIG_HOME=str(root/'config'),XDG_CACHE_HOME=str(root/'cache'),XDG_DATA_HOME=str(root/'data'),XDG_STATE_HOME=str(root/'state'),OTTRIN_RENDERER='opengl',LIBGL_ALWAYS_SOFTWARE='1',WINIT_UNIX_BACKEND='x11',OTTRIN_DEV_HELPER_AUTOBUILD='0')
 fixture=root/'files';fixture.mkdir();(fixture/'sample.txt').write_text('Menu appearance acceptance')
 config=root/'config/ottrin/config.json'
 for name,theme,scale in [('dark-100','Dark','1'),('light-150','Light','1.5')]:
  (fixture/'sample.txt').write_text('Menu appearance acceptance')
  destination=root/('destination-'+name);destination.mkdir()
  if config.exists():
   c=json.loads(config.read_text());c['theme']=theme;c['window_size']=[1000,700];config.write_text(json.dumps(c))
  env['WINIT_X11_SCALE_FACTOR']=scale
  with (out/(name+'-refined-app.log')).open('w') as log:
   app=subprocess.Popen(['dbus-run-session','--',str(binary),'--select',str(fixture/'sample.txt')],env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
   deadline=time.monotonic()+25;win=None
   while time.monotonic()<deadline:
    assert app.poll() is None,'app exited'
    q=run(['xdotool','search','--onlyvisible','--name','Ottrin'],env=env,capture_output=True,text=True)
    if q.returncode==0 and q.stdout.strip():win=q.stdout.splitlines()[0];break
    time.sleep(.2)
   assert win,'missing app window'
   time.sleep(3)
   dims=dict(l.split('=',1) for l in subprocess.check_output(['xdotool','getwindowgeometry','--shell',win],env=env,text=True,timeout=20).splitlines() if '=' in l)
   width=int(dims['WIDTH']);s=float(scale)
   run(['import','-window',win,str(out/(name+'-refined-overview.png'))],env=env,check=True)
   for menu,offset in [('main',32),('view',102)]:
    run(['xdotool','mousemove','--window',win,str(width-int(offset*s)),str(int(58*s)),'click','1'],env=env,check=True);time.sleep(.6)
    run(['import','-window',win,str(out/(name+'-refined-'+menu+'.png'))],env=env,check=True)
    run(['xdotool','windowfocus',win,'key','Escape'],env=env,check=True);time.sleep(.3)
   run(['xdotool','windowfocus',win,'key','ctrl+x'],env=env,check=True);time.sleep(.2)
   run(['xdotool','key','ctrl+l'],env=env,check=True);time.sleep(.2)
   run(['xdotool','key','ctrl+a'],env=env,check=True)
   run(['xdotool','type','--clearmodifiers',str(destination)],env=env,check=True)
   run(['xdotool','key','Return'],env=env,check=True);time.sleep(1)
   started=time.monotonic()
   run(['xdotool','key','ctrl+v'],env=env,check=True);time.sleep(.5)
   run(['import','-window',win,str(out/(name+'-move-complete.png'))],env=env,check=True)
   elapsed=round((time.monotonic()-started)*1000)
   assert not (fixture/'sample.txt').exists(),'Move left source behind'
   assert (destination/'sample.txt').read_text()=='Menu appearance acceptance','Move bytes differ'
   time.sleep(2)
   run(['import','-window',win,str(out/(name+'-move-settled.png'))],env=env,check=True)
   operations.append({'theme':theme,'scale':scale,'capture_elapsed_ms':elapsed,'settled_capture_elapsed_ms':round((time.monotonic()-started)*1000),'source_removed':True,'destination_bytes_match':True})
   run(['xdotool','mousemove','--window',win,str(width-int(22*s)),str(int(18*s)),'click','1'],env=env,check=True)
   assert app.wait(timeout=15)==0
   app=None
finally:
 if app and app.poll() is None:
  os.killpg(app.pid,signal.SIGTERM)
  try:app.wait(timeout=5)
  except subprocess.TimeoutExpired:os.killpg(app.pid,signal.SIGKILL);app.wait()
 for pid in private_pids():
  try:os.kill(pid,signal.SIGTERM)
  except ProcessLookupError:pass
 deadline=time.monotonic()+5
 while private_pids() and time.monotonic()<deadline:time.sleep(.1)
 for pid in private_pids():
  try:os.kill(pid,signal.SIGKILL)
  except ProcessLookupError:pass
 time.sleep(.2)
 if display:
  display.terminate()
  try:display.wait(timeout=5)
  except subprocess.TimeoutExpired:display.kill();display.wait()
 shutil.rmtree(root)

 (out/"LATEST_CLEANUP.json").write_text(json.dumps({"profile_removed":not root.exists(),"private_processes":private_pids(),"ownership":"HOME/XDG environment and uid"},indent=2)+"\n")
 assert not private_pids(), "Private processes remain"

(out/"MOVE_RESULTS.json").write_text(json.dumps(operations,indent=2)+"\n")
