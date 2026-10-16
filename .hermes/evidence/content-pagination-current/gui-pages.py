"""Exact AppImage GUI paging, then controlled-provider cancellation in its owned extraction."""
import csv,hashlib,io,json,os,pathlib,re,shutil,signal,subprocess,sys,tempfile,time
from PIL import Image
artifact=pathlib.Path(sys.argv[1]).resolve();out=pathlib.Path(__file__).parent/'gui';out.mkdir(exist_ok=True)
repo=pathlib.Path.cwd();root=pathlib.Path(tempfile.mkdtemp(prefix='op-gui-',dir=repo/'.hermes/qa'))
app=None;display=None;process_starts={};result={'status':'running','desktop_scale':2.0,'artifact':artifact.name,'sha256':hashlib.sha256(artifact.read_bytes()).hexdigest()}
def run(args,**kw):return subprocess.run(args,timeout=25,**kw)
def cancel(signum,frame):raise SystemExit('cancelled GUI QA')
for sig in [signal.SIGINT,signal.SIGTERM]:signal.signal(sig,cancel)
def own_pids():
 found=[]
 for proc in pathlib.Path('/proc').iterdir():
  try:
   if not proc.name.isdigit() or proc.stat().st_uid!=os.getuid():continue
   values=dict(e.split(b'=',1) for e in (proc/'environ').read_bytes().split(b'\0') if b'=' in e)
   if values.get(b'XDG_STATE_HOME')==os.fsencode(root/'state'):found.append(int(proc.name))
  except OSError:pass
 return found
def wait(check,seconds=20):
 deadline=time.monotonic()+seconds
 while True:
  value=check()
  if value:return value
  if time.monotonic()>deadline:raise TimeoutError('GUI condition did not settle')
  time.sleep(.1)
def key(value):run(['xdotool','windowfocus',win,'key','--clearmodifiers',value],env=env,check=True)
def type_query(value):
 key('ctrl+f');time.sleep(.15);key('ctrl+a')
 if value:run(['xdotool','type','--clearmodifiers','--delay','5',value],env=env,check=True)
 else:key('BackSpace')
def ocr():
 run(['import','-window',win,str(root/'frame.png')],env=env,check=True)
 text=subprocess.check_output(['tesseract',str(root/'frame.png'),'stdout','--psm','11','tsv'],stderr=subprocess.DEVNULL,text=True,timeout=20)
 (out/'latest-ocr.tsv').write_text(text)
 return list(csv.DictReader(io.StringIO(text),delimiter='\t'))
def click_word(word):
 words=ocr();matches=[v for v in words if v['text'].lower().strip('>.<')==word.lower()]
 if not matches:return False
 v=matches[0];run(['xdotool','mousemove','--window',win,str(int(v['left'])+int(v['width'])//2),str(int(v['top'])+int(v['height'])//2),'click','1'],env=env,check=True);return True
def page_ready(offset):
 words=ocr();headers=[w for w in words if w['text'].lower()=='results']
 if not headers:return False
 header=headers[0];x=max(0,int(header['left'])-60);y=int(header['top'])
 names={m for w in words if int(w['left'])>=x and int(w['top'])>y for m in re.findall(r'item-\d\d\.txt',w['text'])}
 expected={f'item-{i:02}.txt' for i in range(offset,min(offset+3,23))}
 return expected.issubset(names) and names.issubset(expected)
def marker_pid():
 try:return json.loads((root/'provider-ready.json').read_text())['pid']
 except (OSError,ValueError,KeyError):return None
def fixture_alive(pid):
 try:
  proc=pathlib.Path('/proc',str(pid));identity=(proc.stat().st_uid,proc.joinpath('stat').read_text().rsplit(')',1)[1].split()[19])
  if pid in process_starts:return identity==process_starts[pid]
  if pid in own_pids() and any(pathlib.Path(os.fsdecode(arg)).resolve()==provider.resolve() for arg in proc.joinpath('cmdline').read_bytes().split(b'\0') if arg):process_starts[pid]=identity;return True
  return False
 except OSError:return False

try:
 run([str(artifact),'--appimage-extract'],cwd=root,stdout=subprocess.DEVNULL,check=True)
 stage=root/'squashfs-root';binary=stage/'AppRun';provider=stage/'usr/libexec/ottrin/rg'
 pin=json.loads((repo/'packaging/search-runtime.json').read_text())['platforms']['linux-x86_64']['binary_sha256'];assert hashlib.sha256(provider.read_bytes()).hexdigest()==pin
 r,w=os.pipe();display=subprocess.Popen(['Xvfb','-displayfd',str(w),'-screen','0','2600x1900x24','-nolisten','tcp'],pass_fds=(w,),stdout=subprocess.DEVNULL,stderr=(out/'xvfb.log').open('w'));os.close(w)
 data=b''
 while not data.endswith(b'\n'):
  block=os.read(r,32);assert block,('Xvfb failed before readiness',display.poll());data+=block
 number=data.decode().strip();assert number.isdigit();os.close(r)
 env=os.environ.copy()
 for name in ['WAYLAND_DISPLAY','LD_LIBRARY_PATH','DBUS_SESSION_BUS_ADDRESS','OTTRIN_INDEX_SOCKET']:env.pop(name,None)
 env.update(WINIT_X11_SCALE_FACTOR='2.0',DISPLAY=':'+number,HOME=str(root),WINIT_UNIX_BACKEND='x11',LIBGL_ALWAYS_SOFTWARE='1',OTTRIN_RENDERER='opengl',OTTRIN_DEV_HELPER_AUTOBUILD='0',OTTRIN_QA_PROVIDER_MARKER=str(root/'provider-ready.json'))
 for name,folder in [('XDG_CONFIG_HOME','config'),('XDG_CACHE_HOME','cache'),('XDG_DATA_HOME','data'),('XDG_STATE_HOME','state'),('XDG_RUNTIME_DIR','runtime')]:
  path=root/folder;path.mkdir(mode=0o700);env[name]=str(path)
 wait(lambda:run(['xdpyinfo'],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode==0,10)
 files=root/'files';files.mkdir()
 for index in range(23):(files/f'item-{index:02}.txt').write_text('bodyneedle')
 config=dict(theme='Dark',default_view_mode='Miller',list_columns=[],command_frame_always_visible=False,restore_session=False,show_hidden_files=False,target={'current':None},last_session_dirs=[],window_size=[1200,900],search_setup_completed=True,search=dict(include_roots=[str(files)],exclude_roots=[],use_system_locate=False,manage_locate_db=False,content_indexing_enabled=True,content_include_roots=[str(files)],content_exclude_roots=[],content_file_groups=['TextDocuments'],result_limit=3,default_sort='Name',refresh_interval_hours=0))
 preference=root/'config/ottrin/config.json';preference.parent.mkdir();preference.write_text(json.dumps(config))
 log=(out/'app.log').open('w');app=subprocess.Popen(['dbus-run-session','--',str(binary),'--select',str(files/'item-00.txt')],env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
 def window():
  assert display.poll() is None, 'Owned Xvfb exited; inspect xvfb.log'
  assert app.poll() is None, 'App exited before first window; inspect app.log'
  q=run(['xdotool','search','--onlyvisible','--name','^Ottrin$'],env=env,capture_output=True,text=True)
  return q.stdout.splitlines()[0] if q.returncode==0 and q.stdout.strip() else None
 win=wait(window,35);time.sleep(1);type_query('bodyneedle');wait(lambda:click_word('Content'))
 wait(lambda:page_ready(0));assert click_word('Previous');time.sleep(.35);assert page_ready(0);shutil.copyfile(root/'frame.png',out/'first-page.png')
 visited=[0]
 for offset in [3,6,9,12,15,18,21]:
  assert click_word('Next');wait(lambda:page_ready(offset));visited.append(offset)
 assert click_word('Next');time.sleep(.35);assert page_ready(21)
 shutil.copyfile(root/'frame.png',out/'last-page.png')
 assert click_word('Previous');wait(lambda:page_ready(18));visited.append(18)
 result['paging']={'total':23,'page_size':3,'visited_offsets':visited,'visible_names_and_previous_next':'pass','provider':'Pinned unmodified private provider'}
 # Only this extraction is changed; the original AppImage stays byte-identical.
 provider.rename(provider.with_name('rg-held'))
 provider.write_text('#!'+sys.executable+'\nimport json,os,sys,time\nsys.stdout.buffer.write(b"x"*1048576);sys.stdout.flush()\nsys.stderr.buffer.write(b"x"*1048576);sys.stderr.flush()\nopen(os.environ["OTTRIN_QA_PROVIDER_MARKER"],"w").write(json.dumps({"pid":os.getpid()}))\ntime.sleep(30)\n');provider.chmod(0o755)
 type_query('slowfirst');first=wait(marker_pid,15);assert fixture_alive(first)
 started=time.monotonic();type_query('slowsecond');second=wait(lambda:marker_pid() if marker_pid()!=first else None,15)
 assert fixture_alive(second)
 wait(lambda:not fixture_alive(first),3);supersession=time.monotonic()-started
 started=time.monotonic();type_query('');wait(lambda:not fixture_alive(second),3);clear=time.monotonic()-started
 (root/'provider-ready.json').unlink();type_query('slowclose');third=wait(marker_pid,15);assert fixture_alive(third)
 geometry=dict(line.split('=',1) for line in subprocess.check_output(['xdotool','getwindowgeometry','--shell',win],env=env,text=True,timeout=20).splitlines() if '=' in line)
 started=time.monotonic();run(['xdotool','mousemove','--window',win,str(int(geometry['WIDTH'])-56),'36','click','1'],env=env,check=True)
 wait(lambda:app.poll() is not None,8);assert app.returncode==0,('GUI did not close cleanly',app.returncode);wait(lambda:not fixture_alive(third),3)
 result['cancellation']={'superseded_provider_reaped_seconds':supersession,'cleared_provider_reaped_seconds':clear,'closed_window_provider_reaped_seconds':time.monotonic()-started,'controlled_provider':'Owned Python process writes 1 MiB to each pipe then sleeps; only the extracted provider was replaced','gui_exit':app.returncode}
 assert hashlib.sha256(artifact.read_bytes()).hexdigest()==result['sha256']
 result['status']='pass'
except BaseException as error:
 result.update(status='fail',error=repr(error))
 if (root/'frame.png').exists():shutil.copyfile(root/'frame.png',out/'failure.png')
 raise
finally:
 if app and app.poll() is None:
  os.killpg(app.pid,signal.SIGTERM)
  try:app.wait(timeout=5)
  except subprocess.TimeoutExpired:os.killpg(app.pid,signal.SIGKILL);app.wait()
 for pid in own_pids():
  try:os.kill(pid,signal.SIGTERM)
  except ProcessLookupError:pass
 until=time.monotonic()+7
 while own_pids() and time.monotonic()<until:time.sleep(.1)
 for pid in own_pids():
  try:os.kill(pid,signal.SIGKILL)
  except ProcessLookupError:pass
 if display:
  display.terminate()
  try:display.wait(timeout=5)
  except subprocess.TimeoutExpired:display.kill();display.wait()
 left=own_pids();shutil.rmtree(root);result['cleanup']={'profile_extraction_removed':not root.exists(),'private_processes':left}
 if left:result['status']='fail'
 (out/'RESULTS.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
