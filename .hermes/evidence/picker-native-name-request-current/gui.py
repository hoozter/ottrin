"""Caller supplies the native name; actual GUI Save must return it without selecting a row."""
from pathlib import Path
import importlib.util,json,hashlib,time,os
r=Path.cwd();e=Path(__file__).resolve().parent;out=e/'linux';assert not out.exists();binary=r/'target/debug/ottrin-app'
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');desktop=importlib.util.module_from_spec(spec);spec.loader.exec_module(desktop)
report={'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'scope':'Actual source chooser NativePath filename request, Save/overwrite exact response and caller-lease acceptance; no package or Windows GUI acceptance'}
try:
 with desktop.OwnedDesktop(out) as d:
  fixture=d.profile/'fixture';fixture.mkdir();filename=os.fsdecode(b'original\x80.txt');original=fixture/filename;original.write_bytes(b'preserve original bytes\n')
  request={'mode':'save','title':'Native caller filename QA','initial_directory':{'encoding':'unix-bytes','units_hex':os.fsencode(fixture).hex()},'filename':{'encoding':'unix-bytes','units_hex':os.fsencode(filename).hex()}}
  (out/'REQUEST.json').write_text(json.dumps(request,indent=2)+'\n')
  app=d.spawn([str(binary),'picker','--cancel-on-disconnect'],'app.stderr',stdin=True,output_name='response.json');app.stdin.write(json.dumps(request)+'\n');app.stdin.flush();deadline=time.monotonic()+20
  while True:
   assert app.poll() is None
   windows=desktop.run(['xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name',request['title']],d.env,False).stdout.split()
   if windows:break
   assert time.monotonic()<deadline;time.sleep(.05)
  window=windows[-1];desktop.run(['xdotool','windowactivate','--sync',window],d.env);time.sleep(.5)
  desktop.run(['import','-window',window,str(out/'initial.png')],d.env)
  desktop.run(['xdotool','mousemove','--window',window,'902','606'],d.env);desktop.run(['xdotool','click','1'],d.env);time.sleep(.4)
  assert app.poll() is None,'Existing file must require fresh overwrite confirmation'
  desktop.run(['import','-window',window,str(out/'confirmation.png')],d.env)
  desktop.run(['xdotool','mousemove','--window',window,'322','354'],d.env);desktop.run(['xdotool','click','1'],d.env)
  app.wait(timeout=5);app.stdin.close();assert app.returncode==0
  response=json.loads((out/'response.json').read_text());assert response['accepted'] and response['paths']==[{'encoding':'unix-bytes','units_hex':os.fsencode(original).hex()}]
  assert original.read_bytes()==b'preserve original bytes\n';assert list(fixture.iterdir())==[original];assert not (d.profile/'runtime/ottrin/index-v1.sock').exists();assert not (d.profile/'config/ottrin/config.json').exists()
  report.update(status='pass',normal_exit=0,response=response,caller_hint_accepted_without_row_selection=True,original_bytes_preserved=True,no_lossy_replacement_file_created=True,no_index_started=True,no_main_config_written=True)
finally:
 report['teardown']=json.loads((out/'TEARDOWN.json').read_text()) if (out/'TEARDOWN.json').exists() else None
 (out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
