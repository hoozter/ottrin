"""Owned X11 desktop/private bus; actual chooser interactions over standard D-Bus API."""
from pathlib import Path
import importlib.util,json,hashlib,time,os
r=Path.cwd();e=Path(__file__).resolve().parent;out=e/'gui-forced-x11';assert not out.exists();out.mkdir();binary=r/'target/debug/ottrin-app'
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');desktop=importlib.util.module_from_spec(spec);spec.loader.exec_module(desktop)
report={'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'scope':'Source backend standard FileChooser D-Bus methods and actual private X11 chooser;fake frontend owns its standard name;not yet installed real portal activation or Windows/package acceptance'}
def wait(predicate):
 until=time.monotonic()+25
 while not predicate():
  assert time.monotonic()<until,'Owned portal GUI readiness timeout';time.sleep(.05)
def get(name):
 try:return json.loads((out/name).read_text())
 except (FileNotFoundError,json.JSONDecodeError):return {}
def ready_gate(name):wait(lambda:get('STAGE.json').get('waiting_for')==name)
def go(name):(out/('GO-'+name)).touch()
def window(d,title):
 found=[]
 def locate():
  found[:]=desktop.run(['xdotool','search','--onlyvisible','--name','^Ottrin.*'+title+'$'],d.env,False).stdout.split();return bool(found)
 wait(locate);return found[-1]
def click(d,w,x,y):
 desktop.run(['xdotool','windowactivate','--sync',w],d.env);desktop.run(['xdotool','mousemove','--window',w,str(x),str(y)],d.env);desktop.run(['xdotool','click','1'],d.env);time.sleep(.25)
def gone(d,title):return not desktop.run(['xdotool','search','--onlyvisible','--name','^Ottrin.*'+title+'$'],d.env,False).stdout.split()
try:
 with desktop.OwnedDesktop(out) as d:
  # A second owned bus avoids any desktop-initiated frontend acquisition. KWin
  # remains on its original private bus; no host session surface is involved.
  bus=d.spawn(['dbus-daemon','--session','--nofork','--print-address=1'],'portal-bus.log',pipe=True);d.env['DBUS_SESSION_BUS_ADDRESS']=d.ready_line(bus)
  fixture=d.profile/'fixture';fixture.mkdir();original=fixture/os.fsdecode(b'original\x80.txt');original.write_bytes(b'preserve portal source\n')
  parent=d.spawn(['xmessage','-title','Owned portal caller','-buttons','Close:0','Owned caller'],'parent.log');parents=[]
  def parent_ready():parents[:]=desktop.run(['xdotool','search','--onlyvisible','--name','^Owned portal caller$'],d.env,False).stdout.split();return bool(parents)
  wait(parent_ready);d.env.update(OTTRIN_PORTAL_QA=str(out.resolve()),OTTRIN_PORTAL_FIXTURE=str(fixture),OTTRIN_PORTAL_PARENT=f'x11:{int(parents[-1]):x}')
  client=d.spawn(['/usr/bin/python3',str(e/'client.py')],'client.log');wait(lambda:get('CLIENT.json').get('status')=='ready')
  d.env.update(WINIT_UNIX_BACKEND='wayland',WAYLAND_DISPLAY='missing-qa-wayland-socket')
  backend=d.spawn([str(binary),'portal'],'backend.log');ready_gate('start');go('start');w=window(d,'Portal Close QA');time.sleep(.4)
  props=desktop.run(['xprop','-id',w,'WM_TRANSIENT_FOR'],d.env).stdout;assert f'0x{int(parents[-1]):x}' in props;report['native_parent_attached']=True
  ready_gate('close-cancel');go('close-cancel');wait(lambda:get('RESPONSES.json').get('cancel',{}).get('code')==1);wait(lambda:gone(d,'Portal Close QA'))
  ready_gate('save');go('save');w=window(d,'Portal native Save QA');time.sleep(.4);desktop.run(['import','-window',w,str(out/'save-initial.png')],d.env);click(d,w,902,606);desktop.run(['import','-window',w,str(out/'save-confirm.png')],d.env);click(d,w,322,354);wait(lambda:get('RESPONSES.json').get('save',{}).get('code')==0)
  uri='file://'+''.join(chr(b) if (b in b'/-._~' or 48<=b<=57 or 65<=b<=90 or 97<=b<=122) else f'%{b:02X}' for b in os.fsencode(original));assert get('RESPONSES.json')['save']['values']['uris']==[uri]
  ready_gate('batch');go('batch');w=window(d,'Portal batch Save QA');time.sleep(.4);desktop.run(['import','-window',w,str(out/'batch.png')],d.env);click(d,w,902,606);wait(lambda:get('RESPONSES.json').get('batch',{}).get('code')==0)
  batch=get('RESPONSES.json')['batch']['values']['uris'];assert batch==[uri.replace('%80.txt','%80%20%281%29.txt'),uri.replace('%80.txt','%80%20%282%29.txt'),uri.rsplit('/',1)[0]+'/second.txt'],batch
  ready_gate('concurrent');go('concurrent');window(d,'Portal concurrent A');window(d,'Portal concurrent B');ready_gate('close-a');go('close-a');wait(lambda:gone(d,'Portal concurrent A'));assert not gone(d,'Portal concurrent B');ready_gate('close-b');go('close-b');wait(lambda:gone(d,'Portal concurrent B'))
  ready_gate('owner-loss');go('owner-loss');window(d,'Portal lost owner QA');ready_gate('disconnect-owner');go('disconnect-owner');wait(lambda:gone(d,'Portal lost owner QA'));ready_gate('reconnect');go('reconnect');window(d,'Portal reconnected QA');ready_gate('close-reconnected');go('close-reconnected');wait(lambda:get('CLIENT_RESULT.json').get('status')=='pass')
  ready_gate('finish')
  def sample():
   fields=Path(f'/proc/{backend.pid}/stat').read_text().split(') ',1)[1].split();rss=int(Path(f'/proc/{backend.pid}/statm').read_text().split()[1])*os.sysconf('SC_PAGE_SIZE');return time.monotonic(),int(fields[11])+int(fields[12]),rss
  first=sample();time.sleep(10);last=sample();report['quiet_backend']={'seconds':last[0]-first[0],'cpu_percent_one_core':100*(last[1]-first[1])/os.sysconf('SC_CLK_TCK')/(last[0]-first[0]),'rss_bytes':last[2],'process_name':Path(f'/proc/{backend.pid}/comm').read_text().strip()};assert report['quiet_backend']['process_name']=='ottrin-portal';assert report['quiet_backend']['cpu_percent_one_core']<1
  go('finish');client.wait(timeout=5);assert client.returncode==0 and backend.poll() is None and parent.poll() is None
  assert original.read_bytes()==b'preserve portal source\n';assert list(fixture.iterdir())==[original];assert not (d.profile/'runtime/ottrin/index-v1.sock').exists();assert not (d.profile/'config/ottrin/config.json').exists();report.update(status='pass',results=get('CLIENT_RESULT.json'),original_bytes_preserved=True,no_files_created_by_batch=True,no_index_started=True,no_main_config_written=True)
finally:
 report['teardown']=get('TEARDOWN.json');(out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
