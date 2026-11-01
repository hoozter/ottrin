"""Finite private-desktop chooser/index checks; cleanup registered before allocation."""
from pathlib import Path
import argparse,hashlib,importlib.util,json,time
r=Path.cwd();e=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');desktop=importlib.util.module_from_spec(spec);spec.loader.exec_module(desktop)
parser=argparse.ArgumentParser();parser.add_argument('--case',choices=['ready','unavailable'],required=True);args=parser.parse_args();out=e/args.case;assert not out.exists()
binary=r/'target/debug/ottrin-app';index_binary=r/'target/debug/ottrin-indexd'
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
report={'case':args.case,'binary_sha256':sha(binary),'index_binary_sha256':sha(index_binary),'scope':'Actual source-debug chooser; not final package/resource acceptance'}
try:
 with desktop.OwnedDesktop(out) as d:
  fixture=d.profile/'fixture';fixture.mkdir();(fixture/'local').mkdir();(fixture/'remote').mkdir()
  (fixture/'local/local.txt').write_text('preserve local\n');(fixture/'remote/report.txt').write_text('preserve report\n')
  initial={str(p.relative_to(fixture)):sha(p) for p in fixture.rglob('*') if p.is_file()};report['initial_files']=initial
  config=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];config.update(theme='Dark',restore_session=False,last_session_dirs=[],installed_office_previews_enabled=False);config['bookmarks']=[];config['search'].update(include_roots=[str(fixture)],content_indexing_enabled=False)
  path=d.profile/'config/ottrin/config.json';path.parent.mkdir(parents=True);path.write_text(json.dumps(config)+'\n')
  socket=d.profile/'runtime/ottrin/index-v1.sock';index=None
  if args.case=='ready':
   index=d.spawn([str(index_binary),'--index-daemon'],'index.log');deadline=time.monotonic()+15
   while not socket.exists():
    assert index.poll() is None;assert time.monotonic()<deadline;time.sleep(.05)
   time.sleep(1)
  request={'mode':'save' if args.case=='ready' else 'open','title':'Indexed chooser QA','initial_directory':str(fixture/'local')}
  (out/'REQUEST.json').write_text(json.dumps(request,indent=2)+'\n')
  app=d.spawn([str(binary),'picker'],'app.stderr',stdin=True,output_name='response.json');app.stdin.write(json.dumps(request));app.stdin.close();deadline=time.monotonic()+30
  while True:
   assert app.poll() is None
   q=desktop.run(['xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','Ottrin'],d.env,False);windows=q.stdout.strip().splitlines()
   if windows:break
   assert time.monotonic()<deadline;time.sleep(.05)
  window=windows[-1];desktop.run(['xdotool','windowactivate','--sync',window],d.env);time.sleep(.7);desktop.run(['import','-window',window,str(out/'initial.png')],d.env)
  data={'profile':str(d.profile),'fixture':str(fixture),'app_pid':app.pid,'app_start_time':Path(f'/proc/{app.pid}/stat').read_text().split(') ',1)[1].split()[19],'window':window,'environment':{k:d.env[k] for k in ['DISPLAY','DBUS_SESSION_BUS_ADDRESS','HOME','XDG_CONFIG_HOME','XDG_CACHE_HOME','XDG_DATA_HOME','XDG_STATE_HOME','XDG_RUNTIME_DIR']}}
  (out/'SESSION.json').write_text(json.dumps(data,indent=2)+'\n');deadline=time.monotonic()+600
  while app.poll() is None and not (out/'STOP').exists():
   assert time.monotonic()<deadline;time.sleep(.2)
  report['normal_exit']=app.poll();assert app.poll()==0
  report['response']=json.loads((out/'response.json').read_text())
  if args.case=='ready':
   assert report['response']['accepted'];assert report['response']['paths']==[str(fixture/'remote/report.txt')]
   assert index.poll() is None and socket.exists();report['existing_index_survived_picker_close']=True
  else:
   assert not report['response']['accepted'];assert not socket.exists();report['index_not_started']=True
  report['final_files']={str(p.relative_to(fixture)):sha(p) for p in fixture.rglob('*') if p.is_file()};assert report['final_files']==initial
  report['status']='passed: normal close and original bytes unchanged'
finally:
 report['teardown']=json.loads((out/'TEARDOWN.json').read_text()) if (out/'TEARDOWN.json').exists() else None
 (out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
