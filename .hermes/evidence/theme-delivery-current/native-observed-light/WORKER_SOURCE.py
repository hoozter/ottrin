"""Exercise the staged AppImage theme preview inside an owned private desktop."""
from pathlib import Path
import importlib.util, json, os, subprocess, sys, time, hashlib
binary,out,mode=Path(sys.argv[1]),Path(sys.argv[2]),sys.argv[3]
r=Path.cwd();spec=importlib.util.spec_from_file_location('qa',r/'scripts/linux-desktop-qa.py');q=importlib.util.module_from_spec(spec);spec.loader.exec_module(q)
report={'status':'running','mode':mode,'cases':[]};app=None
pref=Path(os.environ['XDG_CONFIG_HOME'])/'ottrin/config.json';config=json.loads(pref.read_text())
docs=Path.home()/'Documents';docs.mkdir();fixture=docs/'preserve.txt';fixture.write_bytes(b'Owned theme audit original\n');original=fixture.read_bytes()
config.update(theme=mode,bookmarks=[],default_view_mode='Miller',window_size=[1000,700],graphics_renderer='Native',active_custom_theme='Theme audit',custom_themes=[{'name':'Theme audit','base_preset':'Nord','dark':{'app_radius':12,'border_radius':10,'button_radius':14},'light':{'app_radius':12,'border_radius':10,'button_radius':14}}])
config['search']['include_roots']=[str(docs)];config['search']['content_indexing_enabled']=False
pref.write_text(json.dumps(config)+'\n');(out/'CONFIG_INPUT.json').write_bytes(pref.read_bytes())
def window(name):
 def lookup():
  ids=q.run('xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','^'+name+'$',capture=True,check=False).stdout.split()
  return ids[0] if len(ids)==1 else None
 return q.wait_for(lookup,'unique '+name,timeout=25)
def editor_frame():
 application=q.ottrin_app()
 if application is None:return None
 candidates=[child for child in application if any(n.name=='Save changes' and n.getRoleName()=='push button' for n in q.all_nodes(child))]
 assert len(candidates)<=1,'Theme editor must be a unique application-owned accessible root'
 return candidates[0] if candidates else None
def nodes(name):
 frame=editor_frame();return [] if frame is None else [n for n in q.all_nodes(frame) if n.name==name and q.pyatspi.STATE_SHOWING in n.getState().getStates()]
def click(node):
 assert app.poll() is None and q.run('xdotool','getwindowpid',editor,capture=True).stdout.strip()==str(app.pid)
 assert node.queryAction().doAction(0)
def capture(name):
 q.run('xdotool','windowactivate','--sync',editor)
 time.sleep(.5)
 q.screenshot(out,name)
def selected(label):
 prefixes={'Selected row':'Selected file-row background','Navigation & tabs':'Navigation and tabs share','Title bar':'Title-bar background and window controls'}
 return any(n.name.startswith(prefixes[label]) and q.pyatspi.STATE_SHOWING in n.getState().getStates() for n in q.all_nodes(editor_frame()))

try:
 with (out/'app.log').open('w') as log:
  app=subprocess.Popen([str(binary),str(docs)],stdout=log,stderr=log,start_new_session=True);q.target_process_id=app.pid
  q.wait_for(q.ottrin_app,'owned packaged application',timeout=30)
  main=window('Ottrin');q.wait_for(lambda:q.visible_entry('preserve.txt'),'source fixture')
  q.click_named_control(main,'Menu');q.click_accessible_name('Settings'+chr(0x2026),'push button')
  window('Ottrin Settings');q.click_accessible_name('Appearance','radio button')
  q.click_accessible_name('Edit Theme audit','push button');editor=window('Ottrin Theme Editor')
  q.run('xdotool','windowmove','--sync',editor,'0','0');q.run('xdotool','windowsize','--sync',editor,'1770','1290');q.run('xdotool','windowactivate','--sync',editor)
  report['accessible_roots']=[{'name':n.name,'role':n.getRoleName()} for n in q.ottrin_app()]
  q.wait_for(editor_frame,'editor accessible frame');q.wait_for(lambda:nodes('assets'),'production preview file row')
  capture('editor-overview')
  asset=q.wait_for(lambda:next((n for n in nodes('assets') if n.getRoleName() in ['push button','toggle button']),None),'assets file row')
  report['asset_actions']=[asset.queryAction().getName(i) for i in range(asset.queryAction().nActions)]
  click(asset);capture('after-row-action');report['inspector_after_row']=[{'name':n.name,'role':n.getRoleName()} for n in q.all_nodes(editor_frame()) if n.name]
  q.wait_for(lambda:selected('Selected row'),'selected row inspector');capture('selected-row')
  tab=q.wait_for(lambda:next((n for n in nodes('New tab') if n.getRoleName()=='push button'),None),'preview New tab')
  click(tab);q.wait_for(lambda:selected('Navigation & tabs'),'tab inspector');capture('navigation-tabs')
  controls=[n for n in nodes('Close window') if n.getRoleName()=='push button'];assert len(controls)==2,[(n.name,n.getRoleName()) for n in controls]
  preview_close=max(controls,key=lambda n:n.queryComponent().getExtents(q.pyatspi.DESKTOP_COORDS).y)
  click(preview_close);q.wait_for(lambda:selected('Title bar'),'window control inspector');assert editor_frame() is not None
  capture('titlebar-controls')
  assert fixture.read_bytes()==original
  report.update(status='functional pass; original captures require review',source_preserved=True,preview_close_keeps_editor_open=True,config_after=json.loads(pref.read_text()))
  report['cases']=['Shared production rows visible','Actual assets row selects Selected row','Actual New tab selects Navigation & tabs','Actual preview Close selects Title bar without closing editor']
except BaseException as error:
 report.update(status='failed',error=repr(error));raise
finally:
 if app is not None:
  try:
   os.killpg(app.pid, __import__('signal').SIGTERM)
  except ProcessLookupError: pass
  try: app.wait(timeout=10)
  except subprocess.TimeoutExpired:
   os.killpg(app.pid, __import__('signal').SIGKILL);app.wait(timeout=10)
 report['app_exit']=app.poll() if app is not None else None
 (out/'FUNCTIONAL_RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
