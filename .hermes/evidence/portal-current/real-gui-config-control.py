"""Real installed desktop portal frontend and D-Bus activation, entirely on owned buses/profiles."""
from pathlib import Path
import importlib.util,json,hashlib,time,os,shlex
r=Path.cwd();e=Path(__file__).resolve().parent;out=e/'real-gui';assert not out.exists();out.mkdir();binary=r/'target/debug/ottrin-app'
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');desktop=importlib.util.module_from_spec(spec);spec.loader.exec_module(desktop)
report={'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'scope':'Actual installed xdg-desktop-portal1.18.4 frontend,public FileChooser API,D-Bus backend activation and actual source chooser in owned X11;registration assets are private test files,not yet production setup/package acceptance'}
def wait(predicate):
 until=time.monotonic()+30
 while not predicate():assert time.monotonic()<until,'Owned real portal readiness timeout';time.sleep(.05)
def get(name):
 try:return json.loads((out/name).read_text())
 except (FileNotFoundError,json.JSONDecodeError):return {}
def gate(name):wait(lambda:get('STAGE.json').get('waiting_for')==name)
def go(name):(out/('GO-'+name)).touch()
def window(d,title):
 found=[]
 def locate():found[:]=desktop.run(['xdotool','search','--onlyvisible','--name','^Ottrin.*'+title+'$'],d.env,False).stdout.split();return bool(found)
 wait(locate);return found[-1]
def click(d,w,x,y):
 desktop.run(['xdotool','windowactivate','--sync',w],d.env);desktop.run(['xdotool','mousemove','--window',w,str(x),str(y)],d.env);desktop.run(['xdotool','click','1'],d.env);time.sleep(.25)
def gone(d,title):return not desktop.run(['xdotool','search','--onlyvisible','--name','^Ottrin.*'+title+'$'],d.env,False).stdout.split()
try:
 with desktop.OwnedDesktop(out) as d:
  fixture=d.profile/'fixture';fixture.mkdir();original=fixture/os.fsdecode(b'original\x80.txt');original.write_bytes(b'preserve real portal source\n')
  definitions=d.profile/'data/portal-definitions';definitions.mkdir();(definitions/'ottrin.portal').write_text('[portal]\nDBusName=org.freedesktop.impl.portal.desktop.ottrin\nInterfaces=org.freedesktop.impl.portal.FileChooser;\n')
  service=d.profile/'data/dbus-1/services/org.freedesktop.impl.portal.desktop.ottrin.service';service.parent.mkdir(parents=True);service.write_text('[D-BUS Service]\nName=org.freedesktop.impl.portal.desktop.ottrin\nExec="'+str(binary)+'" portal\n')
  config=d.profile/'config/xdg-desktop-portal/portals.conf';config.parent.mkdir();config.write_text('[preferred]\ndefault=none\norg.freedesktop.impl.portal.FileChooser=ottrin\n')
  d.env['XDG_DESKTOP_PORTAL_DIR']=str(definitions);bus=d.spawn(['dbus-daemon','--session','--nofork','--print-address=1'],'portal-bus.log',pipe=True);d.env['DBUS_SESSION_BUS_ADDRESS']=d.ready_line(bus)
  parent=d.spawn(['xmessage','-title','Owned real portal caller','-buttons','Close:0','Owned real caller'],'parent.log');parents=[]
  def parent_ready():parents[:]=desktop.run(['xdotool','search','--onlyvisible','--name','^Owned real portal caller$'],d.env,False).stdout.split();return bool(parents)
  wait(parent_ready);d.env.update(OTTRIN_PORTAL_QA=str(out.resolve()),OTTRIN_PORTAL_FIXTURE=str(fixture),OTTRIN_PORTAL_PARENT=f'x11:{int(parents[-1]):x}')
  frontend=d.spawn(['/usr/libexec/xdg-desktop-portal','--replace'],'frontend.log');client=d.spawn(['/usr/bin/python3',str(e/'real-client.py')],'client.log');gate('start');go('start');w=window(d,'Real portal Cancel QA');time.sleep(.3);desktop.run(['import','-window',w,str(out/'cancel.png')],d.env)
  gate('close-cancel');go('close-cancel');wait(lambda:gone(d,'Real portal Cancel QA'));gate('save');go('save');w=window(d,'Real portal native Save QA');time.sleep(.4);click(d,w,902,606);desktop.run(['import','-window',w,str(out/'overwrite.png')],d.env);click(d,w,322,354)
  gate('batch');go('batch');w=window(d,'Real portal batch Save QA');time.sleep(.4);desktop.run(['import','-window',w,str(out/'batch.png')],d.env);click(d,w,902,606);wait(lambda:get('CLIENT_RESULT.json').get('status')=='pass')
  result=get('CLIENT_RESULT.json');uris=[u for response in result['responses'].values() if response[0]==0 for u in response[1]['uris']];assert any('original%80.txt' in u for u in uris),uris;assert any('original%80%20%281%29.txt' in u for u in uris),uris
  gate('finish');go('finish');client.wait(timeout=5);assert client.returncode==0 and frontend.poll() is None
  assert original.read_bytes()==b'preserve real portal source\n';assert list(fixture.iterdir())==[original];assert not (d.profile/'runtime/ottrin/index-v1.sock').exists();assert not (d.profile/'config/ottrin/config.json').exists();report.update(status='pass',real_frontend_activated_backend=True,public_responses=result,original_bytes_preserved=True,no_files_created_by_save_or_batch=True,no_index_started=True,no_main_config_written=True)
finally:
 report['teardown']=get('TEARDOWN.json');(out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
