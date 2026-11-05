"""Current production GUI, private daemon and owned files: stopped/error/recovery."""
from pathlib import Path
import hashlib,importlib.util,json,os,socket,stat,struct,subprocess,sys,time
from PIL import ImageGrab
R=Path.cwd();E=Path(__file__).resolve().parent;O=E/'native-source';B=Path.home();app=None;daemon=None
report={'status':'running','themes':[],'scope':'Production release GUI on owned software X11 desktop with private daemon/state; no packaged/native-distribution, Windows or personal-profile acceptance'}
spec=importlib.util.spec_from_file_location('qa',R/'scripts/linux-desktop-qa.py');q=importlib.util.module_from_spec(spec);spec.loader.exec_module(q)
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def diag():
 body=json.dumps({'version':2,'request':{'type':'status'}}).encode()
 with socket.socket(socket.AF_UNIX) as c:
  c.settimeout(3);c.connect(str(endpoint));c.sendall(struct.pack('!I',len(body))+body)
  def read(n):
   b=b''
   while len(b)<n:
    more=c.recv(n-len(b));assert more;b+=more
   return b
  n=struct.unpack('!I',read(4))[0];assert n<1048576
  return json.loads(read(n))['response']['diagnostics']
def ready():
 try:
  v=diag();return v if v['status']=='Ready' and v['indexed_items']==2 else None
 except (OSError,KeyError):return None
def visible(text):return 'label | '+text in q.visible_a11y_nodes()
def window(pattern):
 def find():
  v=q.run('xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name',pattern,capture=True,check=False).stdout.strip().split()
  return v[0] if len(v)==1 else None
 return q.wait_for(find,'owned '+pattern,timeout=20)
def capture(name):
 q.run('xdotool','mousemove','5','5')
 ImageGrab.grab(xdisplay=os.environ['DISPLAY']).save(O/(name+'.png'))
 (O/(name+'.a11y.txt')).write_text('\n'.join(q.visible_a11y_nodes())+'\n')
def close_window(window_id):
 title=q.run('xdotool','getwindowname',window_id,capture=True).stdout.strip()
 assert title in {'Ottrin','Ottrin Settings'},title
 def has_settings_label(frame):
  return any(n.getRoleName()=='label' and n.name.endswith(' Settings') for n in q.all_nodes(frame))
 frames=[n for n in q.all_nodes(q.ottrin_app()) if n.getRoleName()=='frame' and has_settings_label(n)==(title=='Ottrin Settings')]
 assert len(frames)==1,(title,len(frames))
 buttons=[n for n in q.all_nodes(frames[0]) if n.getRoleName()=='push button' and n.name=='Close window' and q.pyatspi.STATE_SHOWING in n.getState().getStates()]
 assert len(buttons)==1,(title,len(buttons))
 assert buttons[0].queryAction().doAction(0),'Exact viewport Close action refused'
def observe(theme,label,text):
 q.click_accessible_name('Manage index service','push button')
 settings=window('^Ottrin Settings$');q.wait_for(lambda:visible(text),text,timeout=20)
 capture(theme+'-'+label+'-services')
 close_window(settings);q.wait_for(lambda:not q.run('xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','^Ottrin Settings$',capture=True,check=False).stdout.strip(),'settings closed')
 main_only=q.run('xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','^Ottrin$',capture=True).stdout.strip()
 any_match=q.run('xdotool','search','--onlyvisible','--pid',str(app.pid),'--name','^Ottrin Settings$',capture=True,check=False).stdout.strip().split()
 assert main_only and main_only in any_match,'Record actual false positive from the superseded any-match query'
 report.setdefault('settings_close_query_control',[]).append({'theme':theme,'phase':label,'settings_window':settings,'settings_closed_exact_pid_and_name':True,'main_window_still_visible':main_only,'superseded_any_match_query_still_finds_main':True})
 capture(theme+'-'+label+'-main')
def start_daemon(label):
 global daemon
 stream=(O/(label+'-daemon.log')).open('w');streams.append(stream)
 daemon=subprocess.Popen([str(R/'target/release/ottrin-indexd')],stdout=stream,stderr=stream)
 q.wait_for(ready,'private Ready2 index',timeout=45)
 assert sha(Path(f'/proc/{daemon.pid}/exe'))==build['index_daemon_sha256']
 return daemon.pid
streams=[]
try:
 build=json.loads((E/'GUI_BUILD_RESULT.json').read_text());assert build['source_revision'].startswith('b9616095') and build['status']=='pass'
 corpus=B/'files';corpus.mkdir();originals={}
 for name in ['one.txt','two.txt']:
  p=corpus/name;p.write_text('Owned source QA original\n');originals[name]=sha(p)
 config=json.loads((R/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config']
 config.update(theme_preset='Ottrin',active_custom_theme=None,custom_themes=[],default_view_mode='Miller',window_size=[900,600])
 config['search'].update(include_roots=[str(corpus)],exclude_globs=[],content_indexing_enabled=False)
 pref=Path(os.environ['XDG_CONFIG_HOME'])/'ottrin/config.json';pref.parent.mkdir(parents=True);pref.write_text(json.dumps(config))
 endpoint=Path(os.environ['XDG_RUNTIME_DIR'])/'ottrin/index-v1.sock'
 for theme in ['Dark','Light']:
  config['theme']=theme;pref.write_text(json.dumps(config))
  if daemon is None:start_daemon(theme+'-initial')
  stream=(O/(theme+'-gui.log')).open('w');streams.append(stream)
  app=subprocess.Popen([str(R/'target/release/ottrin-app'),str(corpus)],stdout=stream,stderr=stream)
  q.target_process_id=app.pid;q.wait_for(q.ottrin_app,'owned source GUI',timeout=30);main=window('^Ottrin$');q.wait_for(lambda:q.visible_entry('one.txt'),'actual file row')
  assert sha(Path(f'/proc/{app.pid}/exe'))==build['gui_sha256']
  observe(theme,'ready','Running — index ready')
  stopped_pid=daemon.pid;daemon.terminate();daemon.wait(timeout=10)
  observe(theme,'stopped','Stopped')
  if endpoint.exists():
   m=endpoint.lstat();assert stat.S_ISSOCK(m.st_mode) and m.st_uid==os.getuid();endpoint.unlink()
  endpoint.write_text('Owned wrong-type endpoint fixture\n');wrong=sha(endpoint)
  observe(theme,'failed','Needs attention')
  assert endpoint.is_file() and sha(endpoint)==wrong;endpoint.unlink()
  recovered_pid=start_daemon(theme+'-recovered');observe(theme,'recovered','Running — index ready')
  close_window(main);assert app.wait(timeout=15)==0
  assert daemon.poll() is None and ready()
  assert all(sha(corpus/name)==digest for name,digest in originals.items())
  report['themes'].append({'theme':theme,'same_gui_pid_across_transitions':app.pid,'stopped_daemon_pid':stopped_pid,'recovered_daemon_pid_survives_gui_close':recovered_pid,'originals_preserved':True,'transitions':['Ready','Stopped','Needs attention','Ready']})
 report.update(status='pass native source GUI transition scope; original captures require direct review',source_revision=build['source_revision'],gui_sha256=build['gui_sha256'],index_daemon_sha256=build['index_daemon_sha256'],captures={p.name:sha(p) for p in O.glob('*.png')})
except BaseException as error:
 report.update(status='failed',error=repr(error));raise
finally:
 for p in [app,daemon]:
  if p is not None and p.poll() is None:
   p.terminate()
   try:p.wait(timeout=10)
   except subprocess.TimeoutExpired:p.kill();p.wait(timeout=10)
 for stream in streams:stream.close()
 (O/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
