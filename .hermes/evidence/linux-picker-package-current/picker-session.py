"""Private actual-picker QA; declarations and teardown precede any user interaction."""
from pathlib import Path
import argparse,hashlib,importlib.util,json,os,subprocess,time
r=Path.cwd();e=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');desktop=importlib.util.module_from_spec(spec);spec.loader.exec_module(desktop)
parser=argparse.ArgumentParser();parser.add_argument('--case',choices=['dark100','light150'],required=True);args=parser.parse_args();out=e/args.case;assert not out.exists()
build=json.loads((e/'BUILD_SOURCE.json').read_text());content=json.loads((e/'PACKAGE_CONTENT_ACCEPTANCE.json').read_text());binary=Path(build['stage'])/'Ottrin-0.1.0-x86_64.AppImage';assert binary.is_file();digest=hashlib.file_digest(binary.open('rb'),'sha256').hexdigest();assert digest==content['artifacts'][binary.name];report={'case':args.case,'source_revision':build['source_revision'],'artifact_sha256':digest,'scope':'Actual checksum-verified AppImage chooser; isolated GUI and bounded resource observations'}
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
try:
 with desktop.OwnedDesktop(out,scale='1.5' if args.case=='light150' else '1') as d:
  fixture=d.profile/'fixture';fixture.mkdir()
  native_directory=fixture / __import__('os').fsdecode(b'native-\x81');native_directory.mkdir()
  native_file=native_directory / __import__('os').fsdecode(b'original-\x80.txt');native_file.write_text('preserve native original\n')
  initial={str(p.relative_to(fixture)):sha(p) for p in fixture.rglob('*') if p.is_file()};report['initial_files']=initial
  config=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];config.update(theme='Light' if args.case=='light150' else 'Dark',restore_session=False,last_session_dirs=[]);config['bookmarks']=[];config['search']['include_roots']=[str(fixture)]
  path=d.profile/'config/ottrin/config.json';path.parent.mkdir(parents=True);path.write_text(json.dumps(config)+'\n');original_config=path.read_bytes()
  request={'mode':'save','title':'Native path chooser QA','initial_directory':{'encoding':'unix-bytes','units_hex':os.fsencode(native_directory).hex()}}
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
  def process_sample():
   values=Path(f'/proc/{app.pid}/stat').read_text().rsplit(') ',1)[1].split();rss=int(Path(f'/proc/{app.pid}/status').read_text().split('VmRSS:')[1].split()[0]);return {'ticks':int(values[11])+int(values[12]),'rss_kib':rss,'time':time.monotonic()}
  time.sleep(2);first=process_sample();time.sleep(8);last=process_sample();report['quiet_idle']={'seconds':last['time']-first['time'],'cpu_percent_one_core':100*(last['ticks']-first['ticks'])/os.sysconf('SC_CLK_TCK')/(last['time']-first['time']),'rss_kib':last['rss_kib'],'scope':'single isolated chooser after settling; software rendering; not whole-system cold-cache or sustained daemon acceptance'}
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
   assert response['accepted'] and response['paths']==[{'encoding':'unix-bytes','units_hex':os.fsencode(native_file).hex()}];report['exact_native_path_bytes_verified']=True
  assert path.read_bytes()==original_config;report['main_config_bytes_unchanged']=True
  preferences=d.profile/'config/ottrin/picker.json';assert preferences.is_file();report['picker_preferences']=json.loads(preferences.read_text())
  report['final_files']={str(p.relative_to(fixture)):sha(p) for p in fixture.rglob('*') if p.is_file()};assert report['final_files']==initial,'Picker changed fixture files'
  report['index_socket_created']=(d.profile/'runtime/ottrin/index-v1.sock').exists();assert not report['index_socket_created']
  report['status']='normal close; fixture files unchanged' if app.poll()==0 else 'stopped; no GUI acceptance'
finally:
 report['teardown']=json.loads((out/'TEARDOWN.json').read_text()) if (out/'TEARDOWN.json').exists() else None
 (out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
