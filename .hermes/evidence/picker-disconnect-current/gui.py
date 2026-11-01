"""Actual caller lease cancellation and unchanged EOF protocol, with owned teardown."""
from pathlib import Path
import importlib.util,json,hashlib,time,os,subprocess
r=Path.cwd();e=Path(__file__).resolve().parent;out=e/'linux';assert not out.exists();binary=r/'target/debug/ottrin-app'
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');desktop=importlib.util.module_from_spec(spec);spec.loader.exec_module(desktop)
report={'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'scope':'Actual debug-source X11 picker stdin lifetime,preference saving and normal EOF-protocol regression;no Windows or package GUI acceptance'}
def window(d,app,title):
 deadline=time.monotonic()+20
 while time.monotonic()<deadline:
  assert app.poll() is None
  found=desktop.run(['xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name',title],d.env,False).stdout.split()
  if found:return found[-1]
  time.sleep(.05)
 raise TimeoutError('Owned chooser did not map')
try:
 with desktop.OwnedDesktop(out) as d:
  fixture=d.profile/'fixture';fixture.mkdir();original=fixture/'original.txt';original.write_bytes(b'unchanged caller fixture\n')
  helper=d.profile/'wm-close';subprocess.run(['cc',str(r/'.hermes/evidence/picker-parent-current/wm-close.c'),'-lX11','-o',str(helper)],check=True)
  request={'title':'Owned caller lease QA','initial_directory':{'encoding':'unix-bytes','units_hex':os.fsencode(fixture).hex()}}
  app=d.spawn([str(binary),'picker','--cancel-on-disconnect'],'lease.stderr',stdin=True,output_name='lease.response.json');app.stdin.write(json.dumps(request)+'\n');app.stdin.flush();w=window(d,app,request['title']);time.sleep(.4);assert app.poll() is None
  desktop.run(['xdotool','windowactivate','--sync',w],d.env)
  for x in [905,820]:
   desktop.run(['xdotool','mousemove','--window',w,str(x),'110'],d.env);desktop.run(['xdotool','click','1'],d.env);time.sleep(.25)
  desktop.run(['import','-window',w,str(out/'before-disconnect.png')],d.env)
  app.stdin.close();app.wait(timeout=5);assert app.returncode==0;assert not json.loads((out/'lease.response.json').read_text())['accepted']
  saved=json.loads((d.profile/'config/ottrin/picker.json').read_text());assert saved['grid'] and saved['preview'],saved
  report['lease_cancel']={'normal_exit':0,'preferences_saved':saved}
  # A caller that violates one-frame framing cancels, rather than supplying a second request.
  request['title']='Owned extra data QA';app=d.spawn([str(binary),'picker','--cancel-on-disconnect'],'extra.stderr',stdin=True,output_name='extra.response.json');app.stdin.write(json.dumps(request)+'\n');app.stdin.flush();window(d,app,request['title']);app.stdin.write('extra');app.stdin.flush();app.wait(timeout=5);app.stdin.close();assert app.returncode==0;assert not json.loads((out/'extra.response.json').read_text())['accepted'];assert 'unexpected data' in (out/'extra.stderr').read_text();report['extra_data_cancel_exit']=0
  # The regular EOF-delimited protocol remains independently usable.
  request['title']='Owned EOF request QA';app=d.spawn([str(binary),'picker'],'ordinary.stderr',stdin=True,output_name='ordinary.response.json');app.stdin.write(json.dumps(request));app.stdin.close();w=window(d,app,request['title']);time.sleep(.4);assert app.poll() is None;desktop.run([str(helper),w],d.env);app.wait(timeout=5);assert app.returncode==0;assert not json.loads((out/'ordinary.response.json').read_text())['accepted'];report['ordinary_eof_not_cancelled_until_native_close']=True
  assert original.read_bytes()==b'unchanged caller fixture\n';assert not (d.profile/'runtime/ottrin/index-v1.sock').exists();assert not (d.profile/'config/ottrin/config.json').exists();report.update(status='pass',original_bytes_preserved=True,no_index_started=True,no_main_config_written=True)
finally:
 report['teardown']=json.loads((out/'TEARDOWN.json').read_text()) if (out/'TEARDOWN.json').exists() else None
 (out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
