"""Owned Xvfb/DBus/KWin session for native exact-AppImage QA; never use the host display."""
from pathlib import Path
import argparse,hashlib,importlib.util,json,os,select,signal,socket,struct,subprocess,time,traceback
root=Path.cwd();e=root/'.hermes/evidence/linux-ci-package-current'
parser=argparse.ArgumentParser();parser.add_argument('--theme',choices=['Dark','Light'],required=True);parser.add_argument('--scale',required=True);parser.add_argument('--name',required=True);args=parser.parse_args()
out=e/args.name;assert not out.exists();out.mkdir();stop=out/'STOP'
extract=root/'.hermes/packaging-tmp/.linux-ci-extract/squashfs-root';assert (extract/'AppRun').is_file()
content=json.loads((e/'PACKAGE_CONTENT_ACCEPTANCE.json').read_text());build=json.loads((e/'BUILD_SOURCE.json').read_text());assert content['source_revision']==build['source_revision']
spec=importlib.util.spec_from_file_location('test_workspace',root/'scripts/test-workspace.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
for sig in [signal.SIGTERM,signal.SIGINT]:signal.signal(sig,m.cancelled)
owned=[];streams=[];report={'source':build['source_revision'],'artifact_sha256':content['artifacts']['Ottrin-0.1.0-x86_64.AppImage'],'theme':args.theme,'scale':args.scale,'normal_app_exit':False}
def spawn(command,env,log,pipe=False):
 f=(out/log).open('w');streams.append(f);p=subprocess.Popen(command,env=env,stdout=subprocess.PIPE if pipe else f,stderr=f,text=True,start_new_session=True);owned.append(p);return p

def line(process):
 ready,_,_=select.select([process.stdout],[],[],10);assert ready,'owned helper readiness timeout';value=process.stdout.readline().strip();assert value,'owned helper exited before readiness';return value

def run(command,env,check=True):return subprocess.run(command,env=env,check=check,text=True,capture_output=True)
def wait(predicate,seconds=20):
 end=time.monotonic()+seconds
 while time.monotonic()<end:
  value=predicate()
  if value:return value
  time.sleep(.1)
 raise RuntimeError('owned GUI condition timed out')
def start_time(pid):return Path(f'/proc/{pid}/stat').read_text().split(') ',1)[1].split()[19]
def cleanup_daemon(runtime):
 path=runtime/'ottrin/index-v1.sock'
 if not path.exists():return None
 with socket.socket(socket.AF_UNIX,socket.SOCK_STREAM) as connection:
  try:connection.connect(str(path))
  except ConnectionRefusedError:return {'stale_socket':True}
  pid,uid,_=struct.unpack('3i',connection.getsockopt(socket.SOL_SOCKET,socket.SO_PEERCRED,12));assert uid==os.getuid()
  entries=Path(f'/proc/{pid}/environ').read_bytes().split(b'\0');assert os.fsencode('XDG_RUNTIME_DIR='+str(runtime)) in entries
  fd=os.pidfd_open(pid)
  try:
   signal.pidfd_send_signal(fd,signal.SIGTERM)
   if not select.select([fd],[],[],10)[0]:signal.pidfd_send_signal(fd,signal.SIGKILL)
   assert select.select([fd],[],[],5)[0],'owned daemon did not stop'
  finally:os.close(fd)
 return {'pid':pid,'peer_uid':uid,'pidfd_exit_observed':True}
def cleanup_profile_children(profile):
 stopped=[];unreadable=0
 for path in Path('/proc').iterdir():
  if not path.name.isdigit() or int(path.name)==os.getpid():continue
  fd=None
  try:
   if path.stat().st_uid!=os.getuid():continue
   fd=os.pidfd_open(int(path.name))
   entries=(path/'environ').read_bytes().split(b'\0')
   if not any(os.fsencode(k+'='+str(profile/v)) in entries for k,v in [('HOME','home'),('XDG_RUNTIME_DIR','runtime')]):continue
   signal.pidfd_send_signal(fd,signal.SIGTERM)
   if not select.select([fd],[],[],5)[0]:signal.pidfd_send_signal(fd,signal.SIGKILL)
   assert select.select([fd],[],[],5)[0],f'owned child {path.name} did not exit'
   stopped.append(int(path.name))
  except PermissionError:unreadable+=1
  except (ProcessLookupError,FileNotFoundError):continue
  finally:
   if fd is not None:os.close(fd)
 return {'stopped':stopped,'unreadable_foreign_processes_preserved':unreadable}
def cleanup_portal_mount(profile):
 mounts=[line for line in Path('/proc/self/mountinfo').read_text().splitlines() if line.split()[4].startswith(str(profile)+'/')]
 for line in mounts:
  target=Path(line.split()[4]);assert target==profile/'runtime/doc' and ' - fuse.portal ' in line and ('user_id='+str(os.getuid())) in line
  tool=__import__('shutil').which('fusermount3') or __import__('shutil').which('fusermount');assert tool
  subprocess.run([tool,'-u',str(target)],check=True)
 assert not any(line.split()[4].startswith(str(profile)+'/') for line in Path('/proc/self/mountinfo').read_text().splitlines())
 return mounts

try:
 with m.isolated_environment(directory='/home/campbell',prefix='.opqa-') as (profile,env):
  report['profile']=str(profile);runtime=profile/'runtime';runtime.mkdir(mode=0o700)
  env.pop('XAUTHORITY',None);env.pop('SESSION_MANAGER',None)
  env.update(XDG_RUNTIME_DIR=str(runtime),NO_AT_BRIDGE='1',LIBGL_ALWAYS_SOFTWARE='1',WINIT_UNIX_BACKEND='x11',WINIT_X11_SCALE_FACTOR=args.scale,KWIN_COMPOSE='N',QT_QPA_PLATFORM='xcb',OTTRIN_DEV_HELPER_AUTOBUILD='0')
  fixture=profile/'fixture'
  for name in ['source','copy','move','stale','long','contact']:(fixture/name).mkdir(parents=True)
  (fixture/'source/note.txt').write_bytes(b'original packaged QA bytes\n')
  (fixture/'contact/contact.vcf').write_bytes(b'BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Ottrin QA\r\nEND:VCARD\r\n')
  (fixture/'long'/('zz very long selected filename '*6+'.txt')).write_bytes(b'long selection original\n')
  config={'theme':args.theme,'theme_preset':'Ottrin','default_view_mode':'List','list_columns':['Name','Kind','Size','Modified'],'command_frame_always_visible':False,'restore_session':False,'show_hidden_files':False,'target':{'current':None},'last_session_dirs':[],'search_setup_completed':True,'graphics_compatibility_notice_seen':True,'window_size':[1100.,800.],'search':{'include_roots':[str(fixture)],'use_system_locate':False,'manage_locate_db':False,'refresh_interval_hours':0,'content_indexing_enabled':False,'active_indexed_volume_roots':[]}}
  config_path=profile/'config/ottrin/config.json';config_path.parent.mkdir();config_path.write_text(json.dumps(config,indent=2)+'\n')
  try:
   xvfb=spawn(['Xvfb','-displayfd','1','-screen','0','1920x1440x24','-nolisten','tcp','-noreset'],env,'xvfb.log',True);env['DISPLAY']=':'+line(xvfb)
   bus=spawn(['dbus-daemon','--session','--nofork','--print-address=1'],env,'dbus.log',True);env['DBUS_SESSION_BUS_ADDRESS']=line(bus)
   wm=spawn(['kwin_x11','--replace'],env,'wm.log');time.sleep(1)
   app=spawn([str(extract/'AppRun'),'--select',str(fixture/'source/note.txt')],env,'app.log')
   def window():
    if app.poll() is not None:raise RuntimeError('App exited during startup: '+str(app.returncode))
    found=run(['xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','^Ottrin$'],env,False)
    return found.stdout.strip().splitlines()[-1:] or None
   wid=wait(window)[0];run(['xdotool','windowactivate','--sync',wid],env);time.sleep(1)
   run(['import','-window',wid,str(out/'initial.png')],env)
   props=run(['xprop','-id',wid,'WM_CLASS','_NET_WM_PID'],env).stdout
   icon=run(['xprop','-id',wid,'_NET_WM_ICON'],env).stdout
   (out/'WINDOW_ICON.json').write_text(json.dumps({'present':'not found' not in icon,'property_bytes':len(icon.encode()),'property_sha256':hashlib.sha256(icon.encode()).hexdigest()},indent=2)+'\n')
   (out/'WINDOW_PROPERTIES.txt').write_text(props)
   session={'profile':str(profile),'fixture':str(fixture),'app_pid':app.pid,'app_start_time':start_time(app.pid),'window':wid,'source_revision':build['source_revision'],'artifact_sha256':report['artifact_sha256'],'theme':args.theme,'scale':args.scale,'environment':{k:env[k] for k in ['DISPLAY','DBUS_SESSION_BUS_ADDRESS','HOME','XDG_CONFIG_HOME','XDG_CACHE_HOME','XDG_DATA_HOME','XDG_STATE_HOME','XDG_RUNTIME_DIR']}}
   (out/'SESSION.json').write_text(json.dumps(session,indent=2)+'\n')
   deadline=time.monotonic()+1200
   while not stop.exists():
    if app.poll() is not None:
     assert app.returncode==0,app.returncode;report['normal_app_exit']=True;break
    assert time.monotonic()<deadline,'native QA session deadline reached';time.sleep(.2)
   if app.poll()==0:report['normal_app_exit']=True
   if config_path.exists():report['persisted_config']=json.loads(config_path.read_text())
   report['final_fixture_sha256']={str(p.relative_to(fixture)):hashlib.file_digest(p.open('rb'),'sha256').hexdigest() for p in fixture.rglob('*') if p.is_file()}
   report['status']='closed' if report['normal_app_exit'] else 'stopped'
  finally:
   for process in reversed(owned[-1:]):m.stop_tests(process)
   report['daemon_cleanup']=cleanup_daemon(runtime)
   for process in reversed(owned[:-1]):m.stop_tests(process)
   report['owned_processes_reaped']=[{'pid':p.pid,'exit':p.poll()} for p in owned]
   report['additional_owned_children_stopped']=cleanup_profile_children(profile)
   report['owned_portal_mounts_unmounted']=cleanup_portal_mount(profile)
   owned.clear()
except BaseException as error:
 report['status']='failed';report['error']=repr(error);report['traceback']=traceback.format_exc()
 raise
finally:
 for process in reversed(owned):m.stop_tests(process)
 for stream in streams:stream.close()
 report['profile_removed']=not Path(report.get('profile','/nonexistent')).exists()
 (out/'TEARDOWN.json').write_text(json.dumps(report,indent=2)+'\n')
