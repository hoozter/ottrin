"""Exercise named native numeric editing/restore/Cancel on the exact AppImage."""
from pathlib import Path
import hashlib,importlib.util,json,os,signal,subprocess,sys,time
root=Path.cwd();binary=Path(sys.argv[1]);out=Path(sys.argv[2]);theme=sys.argv[3];scale=float(sys.argv[4]);docs=Path.home()/'Documents';docs.mkdir();source=docs/'preserve.txt';original=b'owned original\n';source.write_bytes(original);pref=Path(os.environ['XDG_CONFIG_HOME'])/'ottrin/config.json';config=json.loads(pref.read_text());saved=json.loads((root/'.hermes/evidence/appearance-redesign/package-created-theme.json').read_text());saved['name']='Numeric audit'
for variant in ['dark','light']:saved[variant]['app_radius']=0
config.update(theme=theme,custom_themes=[saved],active_custom_theme=saved['name'],window_size=[1000,700],default_view_mode='List');pref.write_text(json.dumps(config)+'\n')
s=importlib.util.spec_from_file_location('qa',root/'scripts/linux-desktop-qa.py');q=importlib.util.module_from_spec(s);s.loader.exec_module(q);cases=[]
def geometry(window):
 text=q.run('xdotool','getwindowgeometry','--shell',window,capture=True).stdout;v=dict(line.split('=',1) for line in text.splitlines() if '=' in line);return [int(v[k]) for k in ['X','Y','WIDTH','HEIGHT']]
def locate(name):return q.wait_for(lambda:q.run('xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','^'+name+'$',capture=True,check=False).stdout.split() or None,'owned '+name)[0]
def controls(name,role):return [n for n in q.all_nodes(q.ottrin_app()) if n.name==name and n.getRoleName()==role and q.pyatspi.STATE_SHOWING in n.getState().getStates()]
def named(name,role):return q.wait_for(lambda:(v[0] if len(v:=controls(name,role))==1 else None),'unique '+name+' '+role)
def inside(node,window,margin):
 b=node.queryComponent().getExtents(q.pyatspi.DESKTOP_COORDS);x,y,w,h=geometry(window);return b.x>=x and b.x+b.width<=x+w and b.y>y+margin and b.y+b.height<y+h-margin

def type_corners(value):
 q.run('xdotool','windowactivate','--sync',editor)
 for _ in range(16):
  node=named('Window corners','spin button')
  if inside(node,editor,round(80*scale)):break
  b=node.queryComponent().getExtents(q.pyatspi.DESKTOP_COORDS);x,y,w,h=geometry(editor);q.run('xdotool','mousemove','--sync',str(b.x+20),str(y+h//2),'click','--repeat','4','--delay','100','5');time.sleep(.25)
 else:raise AssertionError('Named numeric control is not visibly inside the viewport')
 assert named('Window corners','spin button').queryComponent().grabFocus();q.run('xdotool','key','--clearmodifiers','ctrl+a');q.run('xdotool','type','--clearmodifiers',str(value));q.run('xdotool','key','--clearmodifiers','Return');q.wait_for(lambda:named('Window corners','spin button').queryValue().currentValue==value,'actual numeric value '+str(value))
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
  for _ in range(12):
   choices=controls('Edit Numeric audit','push button')
   if len(choices)==1 and inside(choices[0],settings,round(45*scale)):break
   x,y,w,h=geometry(settings);q.run('xdotool','mousemove','--sync',str(x+3*w//4),str(y+h//2),'click','--repeat','4','--delay','100','5');time.sleep(.25)
  else:raise AssertionError('Saved theme edit control is not visible')
  assert choices[0].queryAction().doAction(0);editor=locate('Ottrin Theme Editor');q.run('xdotool','windowmove','--sync',editor,'0','0');q.run('xdotool','windowsize','--sync',editor,str(round(1180*scale)),str(round(860*scale)));time.sleep(.4)
  type_corners(24);q.screenshot(out,'editor-typed-24');cases.append({'case':'named visible numeric typing','value':named('Window corners','spin button').queryValue().currentValue,'role':'spin button','name':'Window corners'})
  reset=named('Restore Window corners','push button');assert inside(reset,editor,round(60*scale));assert reset.queryAction().doAction(0);q.wait_for(lambda:named('Window corners','spin button').queryValue().currentValue==0,'opening value restored');q.screenshot(out,'editor-restored-0');cases.append({'case':'specifically named native restore','name':'Restore Window corners','value':0})
  type_corners(24);q.click_accessible_name('Cancel','push button');q.wait_for(lambda:not q.run('xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','^Ottrin Theme Editor$',capture=True,check=False).stdout.split(),'editor closed');cases.append({'case':'discard edited draft through native Cancel','saved_opening_radius':0})
  assert q.wait_for(close_settings,'unique native Settings close').queryAction().doAction(0);q.wait_for(lambda:not q.run('xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','^Ottrin Settings$',capture=True,check=False).stdout.split(),'Settings closed');q.click_accessible_name('Close window','push button');assert app.wait(timeout=15)==0
  persisted=json.loads(pref.read_text());assert persisted['custom_themes'][0]['dark']['app_radius']==persisted['custom_themes'][0]['light']['app_radius']==0;assert source.read_bytes()==original
 finally:
  try:os.killpg(app.pid,signal.SIGTERM)
  except ProcessLookupError:pass
  try:app.wait(timeout=10)
  except subprocess.TimeoutExpired:os.killpg(app.pid,signal.SIGKILL);app.wait()
(out/'FUNCTIONAL_RESULT.json').write_text(json.dumps({'status':'pass named visible numeric typing/restore/Cancel and normal closure','theme':theme,'scale':scale,'cases':cases,'source_preserved':source.read_bytes()==original,'unsaved_radius_not_persisted':True,'scope':'Exact AppImage/private X11/software native AT-SPI names, values/actions. No complete39control/platform/human acceptance claim'},indent=2)+'\n')
