import os,sys,subprocess,pathlib,time,json,signal,socket,struct,select,argparse,hashlib,importlib.util
args=argparse.ArgumentParser();args.add_argument("--artifact",type=pathlib.Path,required=True);args.add_argument("--theme",default="Dark");args.add_argument("--scale",default="1");args.add_argument("--label",default="dark100");args.add_argument("--fixture-bin",type=pathlib.Path);args.add_argument("--flow",choices=["cleanup","ordinary"],default="cleanup");args.add_argument("--evidence-dir",type=pathlib.Path);args=args.parse_args();args.artifact=args.artifact.resolve(strict=True)
root=pathlib.Path.cwd();out=(args.evidence_dir or root/'.hermes/evidence/copy-activity-current')/args.label;out.mkdir(parents=True,exist_ok=True)
spec=importlib.util.spec_from_file_location('ottrin_test_workspace',root/'scripts/test-workspace.py');isolation=importlib.util.module_from_spec(spec);spec.loader.exec_module(isolation)
state=isolation.isolated_environment(directory=root/'.hermes/qa',prefix='ottrin-ui-');p,env=state.__enter__();app=xvfb=None;peerfd=None;win=None;source_files={}

result={'artifact_sha256':hashlib.file_digest(args.artifact.open('rb'),'sha256').hexdigest(),'theme':args.theme,'scale':args.scale,'flow':args.flow,'status':'incomplete'}
def cancel(signum,frame):raise KeyboardInterrupt()
signal.signal(signal.SIGTERM,cancel)
def run(args,**kw):return subprocess.run(args,check=True,env=env,**kw)
def key(*keys):run(['xdotool','key','--clearmodifiers',*keys]);time.sleep(.3)
def navigate(path):key('ctrl+l');run(['xdotool','type','--clearmodifiers','--delay','1',str(path)]);key('Return');time.sleep(.4)
def failed_check(command):
 result['failed_check']=command
 inventory={}
 for directory in ['source','replacement','destination','moved']:
  base=p/directory
  if not base.exists():continue
  for path in base.rglob('*'):
   if path.is_symlink():inventory[str(path.relative_to(p))]={'symlink':True}
   elif path.is_file():inventory[str(path.relative_to(p))]={'bytes':path.stat().st_size,'sha256':hashlib.file_digest(path.open('rb'),'sha256').hexdigest()}
 result['failure_fixture_inventory']=inventory
 capture=subprocess.run(['import','-window','root',str(out/'failed-check.png')],env=env,capture_output=True,text=True)
 result['failure_screenshot']={'returncode':capture.returncode,'stderr':capture.stderr}
try:
 for n in ['home','config/ottrin','cache','runtime']:(p/n).mkdir(parents=True,exist_ok=True)
 (p/'runtime').chmod(0o700)
 template=json.loads((root/'.hermes/evidence/address-selection-current/VALID_CONFIG_TEMPLATE.json').read_text());template=json.loads(json.dumps(template).replace('/tmp/ottrin-tabs-lw05ipm5',str(p)));template.update(theme=args.theme,default_view_mode='List');template['search'].update(include_roots=[str(p/'destination')],content_indexing_enabled=False);(p/'config/ottrin/config.json').write_text(json.dumps(template))
 for name in ['WAYLAND_DISPLAY','DBUS_SESSION_BUS_ADDRESS','DBUS_SESSION_BUS_PID']:env.pop(name,None)
 env.update(HOME=str(p/'home'),XDG_CONFIG_HOME=str(p/'config'),XDG_CACHE_HOME=str(p/'cache'),XDG_DATA_HOME=str(p/'data'),XDG_STATE_HOME=str(p/'state'),XDG_RUNTIME_DIR=str(p/'runtime'),LIBGL_ALWAYS_SOFTWARE='1',WINIT_X11_SCALE_FACTOR=args.scale,NO_AT_BRIDGE='1')
 r,w=os.pipe();xlog=(out/'xvfb.log').open('w');xvfb=subprocess.Popen(['Xvfb','-displayfd',str(w),'-screen','0','2000x1400x24','-nolisten','tcp'],env=env,pass_fds=[w],stdout=xlog,stderr=xlog,start_new_session=True);os.close(w);assert select.select([r],[],[],10)[0];display=os.read(r,32).decode().strip();os.close(r);env['DISPLAY']=':'+display
 if args.flow=='cleanup':
  assert args.fixture_bin is not None,'Cleanup flow needs its native fixture exporter'
  subprocess.run([str(args.fixture_bin.resolve()),'--exact','copy_activity_cleanup::tests::cleanup_worker_fixture_child','--nocapture'],env={**env,'OTTRIN_COPY_ACTIVITY_FIXTURE':str(p),'OTTRIN_COPY_ACTIVITY_CASE':'prepare-gui'},check=True)
 else:
  for directory in ['source/Folder/nested','replacement','destination','moved']:(p/directory).mkdir(parents=True,exist_ok=True)
  fixtures={'source/note.txt':'first source\n','source/other.txt':'second file\n','source/Folder/nested/leaf.txt':'nested source\n','replacement/note.txt':'replacement source\n'}
  for relative,value in fixtures.items():(p/relative).write_text(value)
  source_files={relative:hashlib.sha256((p/relative).read_bytes()).hexdigest() for relative in fixtures}
  result['source_files_before']=source_files

 subprocess.run([str(args.artifact),'--appimage-extract'],cwd=p,stdout=subprocess.DEVNULL,check=True)
 result['source_note']=(p/'squashfs-root/usr/share/doc/ottrin/SOURCE_REVISION.txt').read_text();launch=p/'squashfs-root/AppRun';alog=(out/'app.log').open('w');app=subprocess.Popen(['dbus-run-session','--',str(launch),str(p/'destination')],env=env,stdout=alog,stderr=alog,start_new_session=True)
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
  if op=='accept':assert result.get('cleanup_workflow') if args.flow=='cleanup' else result.get('file_checks');result['status']='pass';result['visual_checks']=cmd['checks'];print('accepted',flush=True);continue
  if op=='key':key(*cmd['keys'])
  elif op=='type':run(['xdotool','type','--clearmodifiers','--delay','1','--',cmd['text']])
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
   path=out/(cmd['name']+'.png');run(['import','-window','root' if cmd.get('root') else win,str(path)]);print(json.dumps({'screenshot':str(path)}),flush=True)
  elif op=='check':
   assert args.flow=='ordinary'
   try:
    for relative,value in cmd.get('files',{}).items():
     path=p/relative;assert path.resolve().is_relative_to(p) and path.is_file() and not path.is_symlink() and path.read_text()==value,relative
    for relative in cmd.get('absent',[]):
     path=p/relative;assert path.resolve().is_relative_to(p) and not path.exists(),relative
   except AssertionError:
    failed_check(cmd)
    raise
   result.setdefault('file_checks',[]).append({'label':cmd['label'],'files':cmd.get('files',{}),'absent':cmd.get('absent',[])})
  elif op=='backup-change':
   candidate=next((p/'destination').glob('.ottrin-txn-*'))/'candidate';assert candidate.is_file() and not candidate.is_symlink() and candidate.read_text()=='before';result['cancel_preserved_backup']=True;candidate.write_text('changed backup preserved');result['changed_backup']=str(candidate)
  elif op=='backup-restore':
   candidate=pathlib.Path(result['changed_backup']);assert candidate.parent.parent==p/'destination' and candidate.read_text()=='changed backup preserved';candidate.write_text('before')
  elif op=='verify-cleared':
   assert not list((p/'destination').glob('.ottrin-txn-*'));assert not list((p/'state/ottrin/copies').glob('*.journal'));assert not list((p/'state/ottrin/copies').glob('*.cleanup'));assert (p/'destination/item').read_text()=='first';assert (p/'first/item').read_text()=='first';result['cleanup_workflow']='all private stages, journals and manifest absent; visible publication and source preserved'
  elif op=='close':
   g=dict(line.split('=',1) for line in run(['xdotool','getwindowgeometry','--shell',win],capture_output=True,text=True).stdout.splitlines() if '=' in line);scale=float(args.scale);run(['xdotool','mousemove',str(int(g['X'])+int(g['WIDTH'])-int(18*scale)),str(int(g['Y'])+int(18*scale)),'click','1']);app.wait(timeout=10);assert app.returncode==0
  elif op=='geometry':print(run(['xdotool','getwindowgeometry','--shell',win],capture_output=True,text=True).stdout,flush=True)
  else:raise ValueError(op)
  print('done',flush=True)
finally:
 result['normal_app_exit']=app is not None and app.poll()==0
 if app and app.poll() is None:
  os.killpg(app.pid,signal.SIGTERM)
  try:app.wait(timeout=5)
  except subprocess.TimeoutExpired:os.killpg(app.pid,signal.SIGKILL);app.wait(timeout=5)
 if peerfd is None:
  try:
   sock=socket.socket(socket.AF_UNIX);sock.settimeout(1);sock.connect(str(p/'runtime/ottrin/index-v1.sock'));pid,uid,gid=struct.unpack('3i',sock.getsockopt(socket.SOL_SOCKET,socket.SO_PEERCRED,12));sock.close();assert uid==os.getuid() and ('XDG_RUNTIME_DIR='+str(p/'runtime')).encode() in pathlib.Path(f'/proc/{pid}/environ').read_bytes().split(b'\0');peerfd=os.pidfd_open(pid)
  except (FileNotFoundError,ConnectionError,ProcessLookupError):pass
 if peerfd is not None:
  poll=select.poll();poll.register(peerfd,select.POLLIN)
  if not poll.poll(0):signal.pidfd_send_signal(peerfd,signal.SIGTERM)
  if not poll.poll(3000):signal.pidfd_send_signal(peerfd,signal.SIGKILL);assert poll.poll(3000)
  os.close(peerfd)
 if xvfb:
  if xvfb.poll() is None:xvfb.terminate()
  xvfb.wait(timeout=5)
 result['source_files_preserved']=all((p/relative).is_file() and not (p/relative).is_symlink() and hashlib.sha256((p/relative).read_bytes()).hexdigest()==digest for relative,digest in source_files.items())
 if not result['source_files_preserved'] or not result['normal_app_exit']:result['status']='fail'
 state.__exit__(None,None,None);result['cleanup']='Owned app group, daemon pidfd, Xvfb and shared disposable app state removed';(out/'RESULTS.json').write_text(json.dumps(result,indent=2)+'\n')
