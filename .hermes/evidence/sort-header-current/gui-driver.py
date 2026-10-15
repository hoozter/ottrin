import os,sys,subprocess,pathlib,tempfile,time,json,signal,socket,struct,select,argparse,hashlib
args=argparse.ArgumentParser();args.add_argument("--artifact",type=pathlib.Path,required=True);args.add_argument("--theme",default="Dark");args.add_argument("--scale",default="1");args.add_argument("--label",default="dark100");args=args.parse_args();args.artifact=args.artifact.resolve(strict=True)
root=pathlib.Path.cwd();out=root/'.hermes/evidence/sort-header-current'/args.label;out.mkdir(parents=True,exist_ok=True);temp=tempfile.TemporaryDirectory(prefix='ottrin-tabs-');p=pathlib.Path(temp.name);app=xvfb=None;peerfd=None;win=None
result={'artifact_sha256':hashlib.file_digest(args.artifact.open('rb'),'sha256').hexdigest(),'theme':args.theme,'scale':args.scale,'status':'incomplete'}
def cancel(signum,frame):raise KeyboardInterrupt()
signal.signal(signal.SIGTERM,cancel)
def run(args,**kw):return subprocess.run(args,check=True,env=env,**kw)
def key(*keys):run(['xdotool','key','--clearmodifiers',*keys]);time.sleep(.3)
def navigate(path):key('ctrl+l');run(['xdotool','type','--clearmodifiers','--delay','1',str(path)]);key('Return');time.sleep(.4)
try:
 for n in ['home','config/ottrin','cache','runtime','ALPHA','BRAVO','CHARLIE','select']:(p/n).mkdir(parents=True,exist_ok=True)
 (p/'runtime').chmod(0o700)
 for n in ['ALPHA','BRAVO','CHARLIE']:(p/n/(n+'.txt')).write_text(n+' original\n')
 for n in ['alpha.txt','bravo.txt','charlie.txt']:(p/'select'/n).write_text(n+' original\n')
 template=json.loads((root/'.hermes/evidence/address-selection-current/VALID_CONFIG_TEMPLATE.json').read_text());template=json.loads(json.dumps(template).replace('/tmp/ottrin-tabs-lw05ipm5',str(p)));template.update(theme=args.theme,default_view_mode='Miller');template['search'].update(include_roots=[str(p/'select')],content_indexing_enabled=False);(p/'config/ottrin/config.json').write_text(json.dumps(template))
 env=os.environ.copy()
 for name in ['WAYLAND_DISPLAY','DBUS_SESSION_BUS_ADDRESS','DBUS_SESSION_BUS_PID']:env.pop(name,None)
 env.update(HOME=str(p/'home'),XDG_CONFIG_HOME=str(p/'config'),XDG_CACHE_HOME=str(p/'cache'),XDG_DATA_HOME=str(p/'data'),XDG_STATE_HOME=str(p/'state'),XDG_RUNTIME_DIR=str(p/'runtime'),LIBGL_ALWAYS_SOFTWARE='1',WINIT_X11_SCALE_FACTOR=args.scale,NO_AT_BRIDGE='1')
 r,w=os.pipe();xlog=(out/'xvfb.log').open('w');xvfb=subprocess.Popen(['Xvfb','-displayfd',str(w),'-screen','0','2000x1400x24','-nolisten','tcp'],env=env,pass_fds=[w],stdout=xlog,stderr=xlog,start_new_session=True);os.close(w);assert select.select([r],[],[],10)[0];display=os.read(r,32).decode().strip();os.close(r);env['DISPLAY']=':'+display
 subprocess.run([str(args.artifact),'--appimage-extract'],cwd=p,stdout=subprocess.DEVNULL,check=True)
 result['source_note']=(p/'squashfs-root/usr/share/doc/ottrin/SOURCE_REVISION.txt').read_text();launch=p/'squashfs-root/AppRun';alog=(out/'app.log').open('w');app=subprocess.Popen(['dbus-run-session','--',str(launch),str(p/'ALPHA')],env=env,stdout=alog,stderr=alog,start_new_session=True)
 end=time.monotonic()+20
 while time.monotonic()<end and app.poll() is None:
  r=subprocess.run(['xdotool','search','--onlyvisible','--class','ottrin'],env=env,capture_output=True,text=True)
  if r.returncode==0 and r.stdout.strip():win=r.stdout.splitlines()[0];break
  time.sleep(.1)
 assert win is not None,(out/'app.log').read_text()
 sock=socket.socket(socket.AF_UNIX);sock.connect(str(p/'runtime/ottrin/index-v1.sock'));pid,uid,gid=struct.unpack('3i',sock.getsockopt(socket.SOL_SOCKET,socket.SO_PEERCRED,12));sock.close();assert uid==os.getuid() and ('XDG_RUNTIME_DIR='+str(p/'runtime')).encode() in pathlib.Path(f'/proc/{pid}/environ').read_bytes().split(b'\0');peerfd=os.pidfd_open(pid)
 run(['xdotool','windowfocus',win]);time.sleep(.6);print(json.dumps({'ready':True,'window':win,'profile':str(p),'display':env['DISPLAY']}),flush=True)
 for line in sys.stdin:
  cmd=json.loads(line);op=cmd['op']
  if op=='end':break
  if op=='accept':result['status']='pass';result['visual_checks']=cmd['checks'];print('accepted',flush=True);continue
  if op=='key':key(*cmd['keys'])
  elif op=='nav':navigate(p/cmd['path'])
  elif op=='click':

   for mod in cmd.get('mods',[]):run(['xdotool','keydown',mod])
   time.sleep(.1);run(['xdotool','mousemove',str(cmd['x']),str(cmd['y'])]);run(['xdotool','click',str(cmd.get('button',1))]);time.sleep(.2)
   for mod in cmd.get('mods',[]):run(['xdotool','keyup',mod])
   time.sleep(.3)
  elif op=='drag':
   run(['xdotool','mousemove',str(cmd['from'][0]),str(cmd['from'][1]),'mousedown','1']);time.sleep(.2)
   for x,y in cmd['points']:run(['xdotool','mousemove',str(x),str(y)]);time.sleep(.1)
   run(['xdotool','mouseup','1']);time.sleep(.5)
  elif op=='shot':
   path=out/(cmd['name']+'.png');run(['import','-window',win,str(path)]);print(json.dumps({'screenshot':str(path)}),flush=True)
  elif op=='make_dest':(p/cmd['name']).mkdir(exist_ok=True)
  elif op=='verify_copy':
   dest=p/cmd['dest'];actual={x.name:x.read_text() for x in dest.iterdir() if x.is_file()};expected=cmd.get('expected') or {n:n+' original\n' for n in cmd['names']};assert actual==expected,(actual,expected);result.setdefault('copy_checks',[]).append({'dest':cmd['dest'],'files':list(actual),'bytes_match':True});print(json.dumps({'verified':result['copy_checks'][-1]}),flush=True)
  elif op=='geometry':print(run(['xdotool','getwindowgeometry','--shell',win],capture_output=True,text=True).stdout,flush=True)
  else:raise ValueError(op)
  print('done',flush=True)
finally:
 result['normal_app_exit']=app is not None and app.poll()==0
 if app and app.poll() is None:
  os.killpg(app.pid,signal.SIGTERM)
  try:app.wait(timeout=5)
  except subprocess.TimeoutExpired:os.killpg(app.pid,signal.SIGKILL);app.wait(timeout=5)
 if peerfd is not None:
  poll=select.poll();poll.register(peerfd,select.POLLIN)
  if not poll.poll(0):signal.pidfd_send_signal(peerfd,signal.SIGTERM)
  if not poll.poll(3000):signal.pidfd_send_signal(peerfd,signal.SIGKILL);assert poll.poll(3000)
  os.close(peerfd)
 if xvfb:
  if xvfb.poll() is None:xvfb.terminate()
  xvfb.wait(timeout=5)
 temp.cleanup();result['cleanup']='Owned app group, daemon pidfd, Xvfb and private profile removed';(out/'RESULTS.json').write_text(json.dumps(result,indent=2)+'\n')
