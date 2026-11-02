"""Actual candidate chooser, private AT-SPI activation/tree and owned bus loss."""
from pathlib import Path
import argparse,dbus,hashlib,importlib.util,json,os,resource,signal,subprocess,time
resource.setrlimit(resource.RLIMIT_CORE,(0,0))
root=Path.cwd();parser=argparse.ArgumentParser();parser.add_argument('--case',required=True);args=parser.parse_args();assert args.case in ['numeric-role','main-session-bus'];e=Path(__file__).resolve().parent/args.case;e.mkdir(exist_ok=False)
spec=importlib.util.spec_from_file_location('desktop',root/'scripts/linux-desktop-qa/session.py');desktop=importlib.util.module_from_spec(spec);spec.loader.exec_module(desktop)
binary=root/'target/debug/ottrin-app';report={'status':'running','scope':'Temporary native Linux debug AccessKit configuration; not shipped package or screen-reader usability acceptance','binary_sha256':hashlib.file_digest(binary.open('rb'),'sha256').hexdigest()}
def save():(e/'NATIVE_RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
save()
try:
 with desktop.OwnedDesktop(e/'desktop') as d:
  d.env.pop('NO_AT_BRIDGE',None)
  session=dbus.bus.BusConnection(d.env['DBUS_SESSION_BUS_ADDRESS'])
  status=session.get_object('org.a11y.Bus','/org/a11y/bus')
  props=dbus.Interface(status,'org.freedesktop.DBus.Properties');props.Set('org.a11y.Status','IsEnabled',dbus.Boolean(True))
  address=str(dbus.Interface(status,'org.a11y.Bus').GetAddress())
  bus=dbus.bus.BusConnection(address);dbus_control=dbus.Interface(bus.get_object('org.freedesktop.DBus','/org/freedesktop/DBus'),'org.freedesktop.DBus')
  bus_pid=int(dbus_control.GetConnectionUnixProcessID('org.freedesktop.DBus'))
  envs=(Path('/proc')/str(bus_pid)/'environ').read_bytes().split(b'\0');assert os.fsencode('HOME='+str(d.profile/'home')) in envs,'Accessibility bus is not owned'
  report['accessibility']={'enabled':bool(props.Get('org.a11y.Status','IsEnabled')),'bus_pid':bus_pid,'private_bus':True};save()
  fixture=d.profile/'fixture';fixture.mkdir();source=fixture/'proof.txt';source.write_text('Preserve accessible chooser source bytes.\n');original=source.read_bytes()
  if args.case=='main-session-bus':
   config=json.loads((root/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];config.update(theme='Dark',default_view_mode='List',bookmarks=[],installed_office_previews_enabled=False);config['search'].update(include_roots=[str(fixture)],content_indexing_enabled=False)
   cp=d.profile/'config/ottrin/config.json';cp.parent.mkdir(parents=True);cp.write_text(json.dumps(config)+'\n')
   app=d.spawn([str(binary),str(fixture),'--renderer','opengl'],'app.log');title='^Ottrin$'
  else:
   app=d.spawn([str(binary),'picker'],'app.log',stdin=True,output_name='response.json');app.stdin.write(json.dumps({'mode':'open','title':'Accessibility proof','initial_directory':{'encoding':'unix-bytes','units_hex':str(fixture).encode().hex()}}));app.stdin.close();title='Accessibility proof'
  run=lambda cmd: subprocess.run(cmd,env=d.env,text=True,capture_output=True,timeout=15)
  deadline=time.monotonic()+20;window=None
  while time.monotonic()<deadline:
   assert app.poll() is None,'Candidate exited before native window mapping'
   found=run(['xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name',title])
   if found.returncode==0:window=found.stdout.split()[-1];break
   time.sleep(.1)
  assert window,'Native chooser did not map'
  assert run(['xdotool','windowactivate','--sync',window]).returncode==0
  run(['import','-window',window,str(e/'chooser.png')]).check_returncode()
  registry=dbus.Interface(bus.get_object('org.a11y.atspi.Registry','/org/a11y/atspi/accessible/root'),'org.a11y.atspi.Accessible')
  deadline=time.monotonic()+15;owned=[]
  while time.monotonic()<deadline:
   assert app.poll() is None,'Candidate exited before AT-SPI registration'
   children=registry.GetChildren()
   owned=[(str(n),str(p)) for n,p in children if int(dbus_control.GetConnectionUnixProcessID(str(n)))==app.pid]
   if owned:break
   time.sleep(.1)
  report['registered_application_roots']=owned;report['application_pid']=app.pid;save()
  assert owned,'Candidate never registered on the enabled private AT-SPI bus'
  queue=list(owned);seen=set();nodes=[]
  while queue:
   name,path=queue.pop(0)
   if (name,path) in seen:continue
   seen.add((name,path));assert len(seen)<=500,'Tree exceeds bounded observer; not accepted'
   obj=bus.get_object(name,path);accessible=dbus.Interface(obj,'org.a11y.atspi.Accessible');properties=dbus.Interface(obj,'org.freedesktop.DBus.Properties')
   nodes.append({'bus':name,'path':path,'name':str(properties.Get('org.a11y.atspi.Accessible','Name')),'role':int(accessible.GetRole())})
   queue.extend((str(n),str(p)) for n,p in accessible.GetChildren() if str(n)==name)
  (e/'TREE.json').write_text(json.dumps(nodes,indent=2)+'\n');report['tree_nodes']=len(nodes);report['visible_names']=[n['name'] for n in nodes if n['name']];save()
  report['cancel_control_exposed']=any('Cancel' in n['name'] for n in nodes)
  report['visible_file_exposed']=any('proof.txt' in n['name'] for n in nodes)
  save()
  # The exact private dbus-daemon is owned by this HOME and identified over its
  # own D-Bus connection. Kill only this endpoint, never the session/system bus.
  bus.close()
  if args.case=='main-session-bus':
   control=dbus.Interface(session.get_object('org.freedesktop.DBus','/org/freedesktop/DBus'),'org.freedesktop.DBus');bus_pid=int(control.GetConnectionUnixProcessID('org.freedesktop.DBus'))
   assert os.fsencode('HOME='+str(d.profile/'home')) in (Path('/proc')/str(bus_pid)/'environ').read_bytes().split(b'\0');session.close()
  report['lost_bus']='private session' if args.case=='main-session-bus' else 'private AT-SPI';save();os.kill(bus_pid,signal.SIGTERM);time.sleep(2)
  report['after_bus_loss_exit']=app.poll();save()
  if app.poll() is None:
   run(['xdotool','windowactivate','--sync',window,'key','--clearmodifiers','alt+F4' if args.case=='main-session-bus' else 'Escape']).check_returncode()
   report['normal_cancel_exit']=app.wait(timeout=15)
  report['source_bytes_preserved']=source.read_bytes()==original
  report['status']='pass bounded activation/tree/bus-loss/cancel' if report['after_bus_loss_exit'] is None and report.get('normal_cancel_exit')==0 and report['source_bytes_preserved'] and (report['cancel_control_exposed'] or args.case=='main-session-bus') and report['visible_file_exposed'] else 'failed native integration gate'
except BaseException as error:
 report['status']='failed native integration or observer gate';report['error']=repr(error);raise
finally:save()
