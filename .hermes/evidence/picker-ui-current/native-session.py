"""Private actual-picker QA; declarations and teardown precede any user interaction."""
from pathlib import Path
import argparse,hashlib,importlib.util,json,os,subprocess,time
r=Path.cwd();e=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');desktop=importlib.util.module_from_spec(spec);spec.loader.exec_module(desktop)
parser=argparse.ArgumentParser();parser.add_argument('--case',choices=['open','cancel','multi','filter','save','overwrite','folder','light150'],required=True);args=parser.parse_args();out=e/args.case;assert not out.exists()
binary=r/'target/debug/ottrin-app';assert binary.is_file();report={'case':args.case,'binary_sha256':hashlib.file_digest(binary.open('rb'),'sha256').hexdigest(),'scope':'Actual debug-source picker GUI; not package or final resource acceptance'}
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
try:
 with desktop.OwnedDesktop(out,scale='1.5' if args.case=='light150' else '1') as d:
  fixture=d.profile/'fixture';fixture.mkdir();(fixture/'subfolder').mkdir()
  for name,contents in [('alpha.txt','alpha original\n'),('beta.txt','beta original\n'),('existing.txt','preserve existing\n'),('image.png','extension filter fixture\n'),('subfolder/inside.txt','inside original\n')]:
   (fixture/name).write_text(contents)
  initial={str(p.relative_to(fixture)):sha(p) for p in fixture.rglob('*') if p.is_file()};report['initial_files']=initial
  config=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];config.update(theme='Light' if args.case=='light150' else 'Dark',restore_session=False,last_session_dirs=[]);config['bookmarks']=[];config['search']['include_roots']=[str(fixture)]
  path=d.profile/'config/ottrin/config.json';path.parent.mkdir(parents=True);path.write_text(json.dumps(config)+'\n')
  request={'mode':'folder' if args.case=='folder' else 'save' if args.case in ['save','overwrite'] else 'open','title':'Choose a folder' if args.case=='folder' else 'Save a document' if args.case in ['save','overwrite'] else 'Open a document','initial_directory':str(fixture),'multiple':args.case=='multi'}
  if args.case=='save':request['filename']='new.txt'
  if args.case=='overwrite':request['filename']='existing.txt'
  if args.case=='filter':request['filters']=[{'name':'PNG images','patterns':[{'kind':'mime','value':'image/png'}]}]
  (out/'REQUEST.json').write_text(json.dumps(request,indent=2)+'\n');(out/'DECLARED_FILES.json').write_text(json.dumps(initial,indent=2)+'\n')
  started=time.monotonic();app=d.spawn([str(binary),'picker'],'app.stderr',stdin=True,output_name='response.json');app.stdin.write(json.dumps(request));app.stdin.close()
  deadline=time.monotonic()+30
  while True:
   if app.poll() is not None:raise RuntimeError('Picker exited during startup: '+str(app.returncode))
   q=desktop.run(['xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','Ottrin'],d.env,False)
   windows=q.stdout.strip().splitlines()
   if windows:break
   assert time.monotonic()<deadline,'Picker window readiness timeout';time.sleep(.05)
  report['cold_process_window_ready_ms']=round((time.monotonic()-started)*1000,2)
  window=windows[-1];desktop.run(['xdotool','windowactivate','--sync',window],d.env);time.sleep(.7)
  desktop.run(['import','-window',window,str(out/'initial.png')],d.env)
  data={'profile':str(d.profile),'fixture':str(fixture),'app_pid':app.pid,'app_start_time':Path(f'/proc/{app.pid}/stat').read_text().split(') ',1)[1].split()[19],'window':window,'environment':{k:d.env[k] for k in ['DISPLAY','DBUS_SESSION_BUS_ADDRESS','HOME','XDG_CONFIG_HOME','XDG_CACHE_HOME','XDG_DATA_HOME','XDG_STATE_HOME','XDG_RUNTIME_DIR']}}
  (out/'SESSION.json').write_text(json.dumps(data,indent=2)+'\n')
  deadline=time.monotonic()+600
  while app.poll() is None and not (out/'STOP').exists():
   assert time.monotonic()<deadline,'Owned picker QA lease expired';time.sleep(.2)
  report['normal_exit']=app.poll()
  if app.poll()==0:
   response=json.loads((out/'response.json').read_text());report['response']=response
  report['final_files']={str(p.relative_to(fixture)):sha(p) for p in fixture.rglob('*') if p.is_file()};assert report['final_files']==initial,'Picker changed fixture files'
  report['index_socket_created']=(d.profile/'runtime/ottrin/index-v1.sock').exists();assert not report['index_socket_created']
  report['status']='normal close; fixture files unchanged' if app.poll()==0 else 'stopped; no GUI acceptance'
finally:
 report['teardown']=json.loads((out/'TEARDOWN.json').read_text()) if (out/'TEARDOWN.json').exists() else None
 (out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
