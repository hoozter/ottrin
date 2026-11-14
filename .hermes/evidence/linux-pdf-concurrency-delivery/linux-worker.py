"""Exercise named native numeric editing/restore/Cancel on the exact AppImage."""
from pathlib import Path
import hashlib,importlib.util,json,os,signal,subprocess,sys,time
root=Path.cwd();binary=Path(sys.argv[1]);out=Path(sys.argv[2]);theme=sys.argv[3];scale=float(sys.argv[4]);docs=Path.home()/'Documents';docs.mkdir();source=docs/'preserve.txt';original=b'owned original\n';source.write_bytes(original);pref=Path(os.environ['XDG_CONFIG_HOME'])/'ottrin/config.json';config=json.loads(pref.read_text());saved=json.loads((root/'.hermes/evidence/appearance-redesign/package-created-theme.json').read_text());saved['name']='Numeric audit'
for variant in ['dark','light']:saved[variant].update(app_radius=0,border_radius=0,font_scale=1.0)
config.update(theme=theme,custom_themes=[saved],active_custom_theme=saved['name'],window_size=[1000,700],default_view_mode='List');pref.write_text(json.dumps(config)+'\n')
s=importlib.util.spec_from_file_location('qa',root/'scripts/linux-desktop-qa.py');q=importlib.util.module_from_spec(s);s.loader.exec_module(q);cases=[]
def geometry(window):
 text=q.run('xdotool','getwindowgeometry','--shell',window,capture=True).stdout;v=dict(line.split('=',1) for line in text.splitlines() if '=' in line);return [int(v[k]) for k in ['X','Y','WIDTH','HEIGHT']]
def locate(name):return q.wait_for(lambda:q.run('xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','^'+name+'$',capture=True,check=False).stdout.split() or None,'owned '+name)[0]
def controls(name,role):return [n for n in q.all_nodes(q.ottrin_app()) if n.name==name and n.getRoleName()==role and q.pyatspi.STATE_SHOWING in n.getState().getStates()]
def named(name,role):return q.wait_for(lambda:(v[0] if len(v:=controls(name,role))==1 else None),'unique '+name+' '+role)
def inside(node,window,margin):
 b=node.queryComponent().getExtents(q.pyatspi.DESKTOP_COORDS);x,y,w,h=geometry(window);return b.x>=x and b.x+b.width<=x+w and b.y>y+margin and b.y+b.height<y+h-margin

def near(actual, expected):
 return abs(actual-expected)<0.001

def reveal(name,role,margin):
 for _ in range(20):
  node=named(name,role)
  if inside(node,editor,margin):return node
  b=node.queryComponent().getExtents(q.pyatspi.DESKTOP_COORDS)
  x,y,w,h=geometry(editor)
  direction='4' if b.y<y+margin else '5'
  q.run('xdotool','mousemove','--sync',str(b.x+20),str(y+h//2),'click','--repeat','3','--delay','75',direction)
  time.sleep(.25)
 raise AssertionError('Named numeric control is outside the viewport: '+name)

def type_value(name,value):
 q.run('xdotool','windowactivate','--sync',editor)
 node=reveal(name,'spin button',round(70*scale))
 assert node.queryComponent().grabFocus()
 q.run('xdotool','key','--clearmodifiers','ctrl+a')
 q.run('xdotool','type','--clearmodifiers',str(value))
 q.run('xdotool','key','--clearmodifiers','Return')
 q.wait_for(lambda:near(named(name,'spin button').queryValue().currentValue,value),'typed value for '+name)
def select_component(name):
 q.run('xdotool','windowactivate','--sync',editor)
 _,_,width,_=geometry(editor)
 combo_x=round(width*0.79)
 combo_y=round(130*scale)
 if sys.argv[6]=='true':
  control=named('Theme component','combo box')
  assert control.queryAction().doAction(0)
 else:
  q.run('xdotool','mousemove','--window',editor,str(combo_x),str(combo_y))
  time.sleep(.15)
  q.run('xdotool','mousedown','1')
  time.sleep(.08)
  q.run('xdotool','mouseup','1')
 time.sleep(.35)
 for _ in range(12):
  options=q.visible_named_nodes(name)
  actionable=[]
  for option in options:
   try:
    if option.queryAction().nActions>0:actionable.append(option)
   except NotImplementedError:
    pass
  if len(actionable)==1:
   assert actionable[0].queryAction().doAction(0)
   time.sleep(.35)
   return
  q.run('xdotool','mousemove','--window',editor,str(combo_x),str(combo_y+round(100*scale)),'click','5')
  time.sleep(.2)
 q.screenshot(out,'missing-component')
 (out/'VISIBLE_ACCESSIBILITY.json').write_text(json.dumps(q.visible_a11y_nodes(),indent=2)+'\n')
 raise AssertionError('Cannot select component: '+name)
def close_settings():
 node=named('General','radio button')
 for _ in range(12):
  node=node.parent
  if node is None:return None
  matches=[n for n in q.all_nodes(node) if n.name=='Close window' and n.getRoleName()=='push button' and q.pyatspi.STATE_SHOWING in n.getState().getStates()]
  if len(matches)==1:return matches[0]
  assert len(matches)<2
with (out/'app.log').open('w') as log:
 app=subprocess.Popen([str(binary),str(docs)],stderr=log,start_new_session=True);q.target_process_id=app.pid
 try:
  q.wait_for(q.ottrin_app,'owned exact packaged application');main=locate('Ottrin');q.wait_for(lambda:q.visible_entry('preserve.txt'),'source fixture');q.click_named_control(main,'Menu');q.click_accessible_name('Settings'+chr(0x2026),'push button');settings=locate('Ottrin Settings');q.click_accessible_name('Appearance','radio button')
  def open_editor():
   for _ in range(12):
    choices=controls('Edit Numeric audit','push button')
    if len(choices)==1 and inside(choices[0],settings,round(45*scale)):break
    x,y,w,h=geometry(settings);q.run('xdotool','mousemove','--sync',str(x+3*w//4),str(y+h//2),'click','--repeat','4','--delay','100','5');time.sleep(.25)
   else:raise AssertionError('Saved theme edit control is not visible')
   assert choices[0].queryAction().doAction(0);editor=locate('Ottrin Theme Editor');q.run('xdotool','windowmove','--sync',editor,'0','0');q.run('xdotool','windowsize','--sync',editor,str(round(1180*scale)),str(round(860*scale)));time.sleep(.4)
   return editor
  editor=open_editor()
  executable=Path('/proc')/str(app.pid)/'exe'
  assert '/.mount_' in os.readlink(executable)
  with executable.open('rb') as stream:
   assert hashlib.file_digest(stream,'sha256').hexdigest()==sys.argv[5]
  variant=theme.lower()
  current_component='Theme defaults'
  fields=[
   ('Theme defaults','Window corners','app_radius',24),
   ('Theme defaults','Surface corners','border_radius',14),
   ('Theme defaults','Text size','font_scale',1.2),
   ('Buttons & menus','Button corners','button_radius',14),
   ('Buttons & menus','Window border thickness','window_border_width',3.25),
   ('File labels & icons','Neutral text blend','file_label_blend',0.75),
  ]
  def cancel_editor():
   q.click_accessible_name('Cancel','push button')
   q.wait_for(lambda:not q.run('xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','^Ottrin Theme Editor$',capture=True,check=False).stdout.split(),'editor closed')

  for component,name,key,value in fields:
   if current_component!=component:
    if current_component!='Theme defaults':
     cancel_editor()
     editor=open_editor()
    select_component(component)
    current_component=component
   initial=saved[variant][key]
   node=reveal(name,'spin button',round(70*scale))
   assert near(node.queryValue().currentValue,initial)
   type_value(name,value)
   reset=reveal('Restore '+name,'push button',round(55*scale))
   assert reset.queryAction().doAction(0)
   q.wait_for(lambda:near(named(name,'spin button').queryValue().currentValue,initial),'restored '+name)
   cases.append({'component':component,'name':name,'typed':value,'restored':initial})
  type_value('Neutral text blend',0.85)
  cancel_editor()
  assert q.wait_for(close_settings,'unique native Settings close').queryAction().doAction(0);q.wait_for(lambda:not q.run('xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','^Ottrin Settings$',capture=True,check=False).stdout.split(),'Settings closed');q.click_accessible_name('Close window','push button');assert app.wait(timeout=15)==0
  persisted=json.loads(pref.read_text())
  for variant in ('dark','light'):
   for _,_,key,_ in fields:
    assert near(persisted['custom_themes'][0][variant][key],saved[variant][key])
  assert source.read_bytes()==original
 finally:
  try:os.killpg(app.pid,signal.SIGTERM)
  except ProcessLookupError:pass
  try:app.wait(timeout=10)
  except subprocess.TimeoutExpired:os.killpg(app.pid,signal.SIGKILL);app.wait()
(out/'FUNCTIONAL_RESULT.json').write_text(json.dumps({'status':'pass named visible numeric typing/restore/Cancel and normal closure','theme':theme,'scale':scale,'cases':cases,'source_preserved':source.read_bytes()==original,'unsaved_numeric_edit_not_persisted':True,'named_selector_used':sys.argv[6]=='true','scope':'Exact AppImage/private X11/software native AT-SPI names, values/actions. No complete39control/platform/human acceptance claim'},indent=2)+'\n')
