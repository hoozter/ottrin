"""Finite actual-AppImage main-window acceptance; owned private desktop and teardown."""
from pathlib import Path
import argparse,hashlib,importlib.util,json,os,socket,struct,time
r=Path.cwd();e=Path(__file__).resolve().parent
out=e/'native-gui';assert not out.exists()
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');desktop=importlib.util.module_from_spec(spec);spec.loader.exec_module(desktop)
image=r/'target/debug/ottrin-app'
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
report={'binary_sha256':sha(image),'scope':'Actual source native-filename preview title,VCF and Inspector/Space on owned X11; shared loading and ready title function'}
try:
 with desktop.OwnedDesktop(out,scale='1') as d:
  fixture=d.profile/'fixture';fixture.mkdir();native_source=fixture/os.fsdecode(b'source\x80');native_source.mkdir();(native_source/'note.txt').write_bytes(b'Preserve read-only preview.\n'*20);(native_source/os.fsdecode(b'contact\x80.vcf')).write_bytes(b'BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Ottrin QA\r\nEND:VCARD\r\n');(fixture/'destination').mkdir();(fixture/'destination/original.txt').write_bytes(b'preserve destination\n')
  initial={str(p.relative_to(fixture)):sha(p) for p in fixture.rglob('*') if p.is_file()};report['initial_files']=initial
  config=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];config.update(theme='Dark',default_view_mode='List',restore_session=False,last_session_dirs=[],installed_office_previews_enabled=False);config['bookmarks']=[];config['search'].update(include_roots=[str(fixture)],content_indexing_enabled=False)
  path=d.profile/'config/ottrin/config.json';path.parent.mkdir(parents=True);path.write_text(json.dumps(config)+'\n')
  app=d.spawn([str(image),str(native_source),'--renderer','opengl'],'app.log');started=time.monotonic();deadline=started+15
  while True:
   assert app.poll() is None
   windows=desktop.run(['xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','^Ottrin$'],d.env,False).stdout.strip().splitlines()
   if windows:break
   assert time.monotonic()<deadline;time.sleep(.05)
  report['window_mapped_ms']=round((time.monotonic()-started)*1000,2);window=windows[-1];desktop.run(['xdotool','windowactivate','--sync',window],d.env);time.sleep(1);desktop.run(['import','-window',window,str(out/'initial.png')],d.env)
  socket_path=d.profile/'runtime/ottrin/index-v1.sock';deadline=time.monotonic()+35
  while not socket_path.exists():assert time.monotonic()<deadline;assert app.poll() is None;time.sleep(.1)
  def peer():
   with socket.socket(socket.AF_UNIX) as c:
    c.connect(str(socket_path));pid,uid,gid=struct.unpack('3i',c.getsockopt(socket.SOL_SOCKET,socket.SO_PEERCRED,12));assert uid==os.getuid();return pid
  daemon=peer();start_ticks=Path(f'/proc/{daemon}/stat').read_text().rsplit(') ',1)[1].split()[19];report['daemon_pid']=daemon
  data={'profile':str(d.profile),'fixture':str(fixture),'app_pid':app.pid,'app_start_time':Path(f'/proc/{app.pid}/stat').read_text().rsplit(') ',1)[1].split()[19],'window':window,'environment':{k:d.env[k] for k in ['DISPLAY','DBUS_SESSION_BUS_ADDRESS','HOME','XDG_CONFIG_HOME','XDG_CACHE_HOME','XDG_DATA_HOME','XDG_STATE_HOME','XDG_RUNTIME_DIR']}}
  (out/'SESSION.json').write_text(json.dumps(data,indent=2)+'\n')
  desktop.run(['xdotool','mousemove','--window',window,'130','114','click','1','key','space'],d.env);time.sleep(.6)
  inspector=desktop.run(['xdotool','search','--onlyvisible','--name','^Ottrin Quick Inspector'],d.env).stdout.split()[-1]
  desktop.run(['import','-window',inspector,str(out/'native-inspector.png')],d.env)
  desktop.run(['xdotool','windowactivate','--sync',inspector,'key','space'],d.env);time.sleep(.2)
  desktop.run(['xdotool','windowactivate','--sync',window,'mousemove','--window',window,'1072','18','click','1'],d.env)
  deadline=time.monotonic()+5
  while app.poll() is None:assert time.monotonic()<deadline;time.sleep(.05)
  assert app.poll()==0,'Normal visible Close required';report['normal_exit']=0
  assert peer()==daemon and Path(f'/proc/{daemon}/stat').read_text().rsplit(') ',1)[1].split()[19]==start_ticks;report['index_survived_window_close']=True
  report['persisted_config']=json.loads(path.read_text());assert report['persisted_config']['theme']==config['theme'];report['final_files']={str(p.relative_to(fixture)):sha(p) for p in fixture.rglob('*') if p.is_file()};assert report['final_files']==initial
  report['status']='pass: native directory mapped,native VCF Inspector opened and closed,normal main close,index continuity,saved theme and unchanged declared files'
finally:
 report['teardown']=json.loads((out/'TEARDOWN.json').read_text()) if (out/'TEARDOWN.json').exists() else None
 (out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
