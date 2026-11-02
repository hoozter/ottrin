"""Actual candidate chooser, private AT-SPI activation/tree and owned bus loss."""
from pathlib import Path
import argparse,dbus,hashlib,importlib.util,json,os,resource,signal,subprocess,time
resource.setrlimit(resource.RLIMIT_CORE,(0,0))
root=Path.cwd();parser=argparse.ArgumentParser();parser.add_argument('--case',required=True);args=parser.parse_args();assert args.case in ['numeric-role','main-session-bus','main-client-close'];e=Path(__file__).resolve().parent/args.case;e.mkdir(exist_ok=False)
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
  if args.case.startswith('main-'):
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
  expected=['Back','Forward','Parent folder','Close window','Minimize window','Maximize or restore window'] if args.case.startswith('main-') else ['Parent folder','Edit location','Grid view','List view','Show hidden files','Show read-only preview','Close window']
  names={node['name']:node for node in nodes if node['name']};report['expected_names']=expected
  assert all(name in names for name in expected),('Missing native control names',set(expected)-names.keys())
  import gi
  gi.require_version("Atspi","2.0")
  from gi.repository import Atspi
  def state(name):
   node=names[name];values=dbus.Interface(bus.get_object(node['bus'],node['path']),'org.a11y.atspi.Accessible').GetState()
   mask=sum(int(value)<<(32*i) for i,value in enumerate(values))
   return {'enabled':bool(mask&(1<<int(Atspi.StateType.ENABLED))),'sensitive':bool(mask&(1<<int(Atspi.StateType.SENSITIVE)))}
  report['control_states']={name:state(name) for name in expected}
  report['glyph_only_buttons']=[node for node in nodes if node['role']==int(Atspi.Role.PUSH_BUTTON) and any(0xe000<=ord(c)<=0xf8ff for c in node['name'])]
  assert not report['glyph_only_buttons'],'Visible buttons still expose icon-font text'
  save()
  assert report['control_states']['Parent folder']['enabled']
  if args.case.startswith('main-'):
   assert not report['control_states']['Forward']['enabled'] and not report['control_states']['Forward']['sensitive'],'Disabled Forward became actionable'
  def invoke(name):
   node=names[name];action=dbus.Interface(bus.get_object(node['bus'],node['path']),'org.a11y.atspi.Action');assert len(action.GetActions())>0,(name,'No native action');assert action.DoAction(0),(name,'Native action rejected')
  if not args.case.startswith('main-'):
   invoke('Grid view');time.sleep(.4);run(['import','-window',window,str(e/'grid-action.png')]).check_returncode()
   invoke('List view');time.sleep(.4);run(['import','-window',window,str(e/'list-action.png')]).check_returncode()
   report['native_view_actions_accepted']=True
  invoke('Close window');report['native_close_exit']=app.wait(timeout=15)
  report['source_bytes_preserved']=source.read_bytes()==original
  report['status']='pass native names, disabled states and real accessibility actions' if report['native_close_exit']==0 and report['source_bytes_preserved'] else 'failed native controls'

except BaseException as error:
 report['status']='failed native integration or observer gate';report['error']=repr(error);raise
finally:save()
