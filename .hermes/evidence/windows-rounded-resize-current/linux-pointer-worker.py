from pathlib import Path
import importlib.util,json,os,signal,subprocess,sys,time
root=Path.cwd();binary=Path(sys.argv[1]);out=Path(sys.argv[2]);theme=sys.argv[3];scale=float(sys.argv[4]);home=Path.home();docs=home/'Documents';docs.mkdir();source=docs/'preserve.txt';source.write_bytes(b'owned original\n');pref=Path(os.environ['XDG_CONFIG_HOME'])/'ottrin/config.json';base=json.loads(pref.read_text());saved=json.loads((root/'.hermes/evidence/appearance-redesign/package-created-theme.json').read_text());saved['name']='Rounded gesture';cases=[]
s=importlib.util.spec_from_file_location('qa',root/'scripts/linux-desktop-qa.py');q=importlib.util.module_from_spec(s);s.loader.exec_module(q)
def geometry(window):
 text=q.run('xdotool','getwindowgeometry','--shell',window,capture=True).stdout
 v=dict(line.split('=',1) for line in text.splitlines() if '=' in line);return [int(v[k]) for k in ['X','Y','WIDTH','HEIGHT']]
def locate(pid,name):return q.wait_for(lambda:q.run('xdotool','search','--all','--onlyvisible','--pid',str(pid),'--name','^'+name+'$',capture=True,check=False).stdout.split() or None,'owned '+name)[0]
def gesture(window,label,radius,corner,dx,dy):
 q.run('xdotool','windowactivate','--sync',window);q.run('xdotool','windowmove','--sync',window,'100','100');time.sleep(.5);before=geometry(window);offset=round(max(radius,4)*scale);x=before[2]-offset if corner=='SE' else offset;y=before[3]-round(4*scale) if corner=='SE' else round(4*scale);sx,sy=before[0]+x,before[1]+y
 q.run('xdotool','mousemove','--sync',str(sx),str(sy));time.sleep(.5);q.run('xdotool','mousedown','1')
 try:time.sleep(.5);q.run('xdotool','mousemove','--sync',str(sx+dx),str(sy+dy));time.sleep(.7)
 finally:q.run('xdotool','mouseup','1')
 time.sleep(.5);after=geometry(window);q.screenshot(out,label+'-'+str(radius));record={'case':label,'radius':radius,'scale':scale,'before':before,'after':after,'pointer_start':[x,y],'pointer_delta':[dx,dy]};cases.append(record);(out/'LIVE_CASES.json').write_text(json.dumps(cases,indent=2)+'\n')
 if corner=='SE':assert abs(after[2]-before[2]-dx)<=3 and abs(after[3]-before[3]-dy)<=3,record;assert abs(after[0]-before[0])<=3 and abs(after[1]-before[1])<=3,record
 else:assert abs(after[0]-before[0]-dx)<=3 and abs(after[1]-before[1]-dy)<=3,record;assert abs(after[2]-before[2]+dx)<=3 and abs(after[3]-before[3]+dy)<=3,record
for radius in [0,24]:
 config=json.loads(json.dumps(base));custom=json.loads(json.dumps(saved));custom['light']['app_radius']=radius;custom['dark']['app_radius']=radius;config.update(theme=theme,custom_themes=[custom],active_custom_theme=custom['name'],window_size=[1000,650],default_view_mode='List');pref.write_text(json.dumps(config)+'\n')
 with (out/('app-'+str(radius)+'.log')).open('w') as log:
  app=subprocess.Popen([str(binary),str(docs)],stderr=log,start_new_session=True);q.target_process_id=app.pid
  try:
   q.wait_for(q.ottrin_app,'owned packaged application');window=locate(app.pid,'Ottrin');q.wait_for(lambda:q.visible_entry('preserve.txt'),'owned source fixture');q.run('xdotool','windowsize','--sync',window,str(round(1000*scale)),str(round(650*scale)));time.sleep(.7);gesture(window,'main-visible-arc-SE',radius,'SE',round(40*scale),round(30*scale))
   q.click_named_control(window,'Menu');q.click_accessible_name('Settings'+chr(0x2026),'push button');settings=locate(app.pid,'Ottrin Settings');gesture(settings,'settings-titlebar-NW',radius,'NW',round(20*scale),round(20*scale))
   def close_settings():
    matches=[n for n in q.all_nodes(q.ottrin_app()) if n.name=='General' and n.getRoleName()=='radio button' and q.pyatspi.STATE_SHOWING in n.getState().getStates()]
    if len(matches)!=1:return None
    node=matches[0]
    for _ in range(12):
     node=node.parent
     if node is None:return None
     choices=[n for n in q.all_nodes(node) if n.name=='Close window' and n.getRoleName()=='push button' and q.pyatspi.STATE_SHOWING in n.getState().getStates()]
     if len(choices)==1:return choices[0]
     assert len(choices)<2
   assert q.wait_for(close_settings,'unique settings native close').queryAction().doAction(0);q.wait_for(lambda:not q.run('xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','^Ottrin Settings$',capture=True,check=False).stdout.split(),'settings closed');q.click_accessible_name('Close window','push button');assert app.wait(timeout=15)==0;assert source.read_bytes()==b'owned original\n'
  finally:
   try:os.killpg(app.pid,signal.SIGTERM)
   except ProcessLookupError:pass
   try:app.wait(timeout=10)
   except subprocess.TimeoutExpired:os.killpg(app.pid,signal.SIGKILL);app.wait()
(out/'FUNCTIONAL_RESULT.json').write_text(json.dumps({'status':'pass native pointer resize andexclusive titlebarownership','theme':theme,'scale':scale,'cases':cases,'source_bytes_preserved':source.read_bytes()==b'owned original\n','scope':'FouractualX11mousegestures,0/24radius,mainSE/SettingsNW,normalnativeclose; not wholeappearanceorhardwareacceptance'},indent=2)+'\n')
