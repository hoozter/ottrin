"""Actual candidate chooser, private AT-SPI activation/tree and owned bus loss."""
from pathlib import Path
import argparse,dbus,hashlib,importlib.util,json,os,resource,signal,subprocess,time
resource.setrlimit(resource.RLIMIT_CORE,(0,0))
root=Path.cwd();parser=argparse.ArgumentParser();parser.add_argument('--case',required=True);parser.add_argument('--output',type=Path);args=parser.parse_args();assert args.case in ['main-modals'];e=args.output.resolve() if args.output else Path(__file__).resolve().parent/args.case;assert e.is_relative_to(root/'.hermes/evidence');e.mkdir(exist_ok=False)
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
  main_close={(n['bus'],n['path']) for n in nodes if n['name']=='Close window'}
  def snapshot(label):
   queue=[(str(n),str(p)) for n,p in registry.GetChildren() if int(dbus_control.GetConnectionUnixProcessID(str(n)))==app.pid];seen=set();result=[]
   while queue:
    name,path=queue.pop(0)
    if (name,path) in seen:continue
    seen.add((name,path));assert len(seen)<800,'Observer tree bound exceeded'
    obj=bus.get_object(name,path);acc=dbus.Interface(obj,'org.a11y.atspi.Accessible');prop=dbus.Interface(obj,'org.freedesktop.DBus.Properties')
    result.append({'bus':name,'path':path,'name':str(prop.Get('org.a11y.atspi.Accessible','Name')),'role':int(acc.GetRole())})
    queue.extend((str(n),str(p)) for n,p in acc.GetChildren() if str(n)==name)
   (e/(label+'-tree.json')).write_text(json.dumps(result,indent=2)+'\n');return result
  def wait_snapshot(label,predicate):
   deadline=time.monotonic()+15
   while True:
    items=snapshot(label)
    if predicate(items):return items
    assert app.poll() is None,'Application exited while observing native tree'
    assert time.monotonic()<deadline,'Native tree did not reach declared state: '+label
    time.sleep(.1)
  def node_named(items,name):
   found=[n for n in items if n['name']==name];assert len(found)==1,(name,len(found));return found[0]
  def invoke_node(node):
   act=dbus.Interface(bus.get_object(node['bus'],node['path']),'org.a11y.atspi.Action');assert act.DoAction(0);time.sleep(.4)
  def child_window(title):
   deadline=time.monotonic()+15
   while time.monotonic()<deadline:
    p=run(['xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name',title])
    if p.returncode==0:return p.stdout.split()[-1]
    assert app.poll() is None;time.sleep(.1)
   raise AssertionError('Owned modal did not map: '+title)
  def close_modal(items,win):
   close=[n for n in items if n['name']=='Close window' and (n['bus'],n['path']) not in main_close];assert len(close)==1,close
   invoke_node(close[0]);deadline=time.monotonic()+10
   while time.monotonic()<deadline:
    if run(['xdotool','getwindowname',win]).returncode!=0:break
    time.sleep(.1)
   else:raise AssertionError('Native modal Close did not close child')
   assert app.poll() is None,'Child Close terminated main'
  invoke('Menu');menu=wait_snapshot('settings-menu',lambda items:any(n['name']=='Settings…' for n in items));invoke_node(node_named(menu,'Settings…'));settings=child_window('^Ottrin Settings$');run(['import','-window',settings,str(e/'settings.png')]).check_returncode();settings_nodes=wait_snapshot('settings',lambda items:any(n['name']=='Close window' and (n['bus'],n['path']) not in main_close for n in items));close_modal(settings_nodes,settings);report['settings_native_open_close']=True;save()
  items=snapshot('main-after-settings');file=node_named(items,'proof.txt');rect=dbus.Interface(bus.get_object(file['bus'],file['path']),'org.a11y.atspi.Component').GetExtents(int(Atspi.CoordType.SCREEN));x,y,w,h=map(int,rect);assert w>0 and h>0
  run(['xdotool','windowactivate','--sync',window,'mousemove',str(x+w//2),str(y+h//2),'click','1']).check_returncode();time.sleep(.3)
  invoke_node(node_named(items,'Menu'));menu=wait_snapshot('inspector-menu',lambda items:any(n['name']=='Quick Inspector Selection' for n in items));invoke_node(node_named(menu,'Quick Inspector Selection'));inspector=child_window('^Ottrin Quick Inspector');run(['import','-window',inspector,str(e/'inspector.png')]).check_returncode();inspector_nodes=wait_snapshot('inspector',lambda items:any(n['name']=='Close window' and (n['bus'],n['path']) not in main_close for n in items))
  inspector_glyph_buttons=[n for n in inspector_nodes if n['role']==int(Atspi.Role.PUSH_BUTTON) and any(0xe000<=ord(c)<=0xf8ff for c in n['name'])];report['inspector_glyph_buttons']=inspector_glyph_buttons;save();assert not inspector_glyph_buttons,'Inspector buttons still expose icon-font characters'
  assert all(any(n['name']==expected for n in inspector_nodes) for expected in ['Edit text','More actions']),'Inspector action labels are missing'
  close=[n for n in inspector_nodes if n['name']=='Close window' and (n['bus'],n['path']) not in main_close];assert len(close)==1
  prefix=close[0]['path'].rsplit('/',1)[0]+'/'
  editable=[]
  for node in inspector_nodes:
   if node['path'].startswith(prefix):
    interfaces=[str(v) for v in dbus.Interface(bus.get_object(node['bus'],node['path']),'org.a11y.atspi.Accessible').GetInterfaces()]
    if 'org.a11y.atspi.EditableText' in interfaces:editable.append(node)
  report['inspector_editable_nodes']=editable;assert not editable,'Read-only Inspector exposes editable-text interface'
  close_modal(inspector_nodes,inspector);report['inspector_native_open_close']=True
  items=snapshot('main-after-inspector');invoke_node(node_named(items,'Close window'));report['native_close_exit']=app.wait(timeout=15);report['source_bytes_preserved']=source.read_bytes()==original
  report['status']='pass native Linux Settings/Inspector actions and read-only interface check' if report['native_close_exit']==0 and report['source_bytes_preserved'] else 'failed native modal controls'

except BaseException as error:
 report['status']='failed native integration or observer gate';report['error']=repr(error);raise
finally:save()
