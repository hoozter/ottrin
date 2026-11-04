"""Exercise actual Ottrin Services and a stock graphical polkit agent in guest X11."""
from pathlib import Path
import hashlib,importlib.util,json,os,subprocess,time
from PIL import ImageGrab
base=Path.home();out=base/'results';report={'status':'running','cases':[]};children=[]
spec=importlib.util.spec_from_file_location('qa',base/'linux-desktop-qa.py');q=importlib.util.module_from_spec(spec);spec.loader.exec_module(q)
def capture(name):ImageGrab.grab(xdisplay=os.environ['DISPLAY']).save(out/(name+'.png'))
def auth_nodes():
 desktop=q.pyatspi.Registry.getDesktop(0)
 for application in desktop:
  if application is not None and application.get_process_id()==agent.pid:
   yield from q.all_nodes(application)
def auth_button(name):
 return next((n for n in auth_nodes() if n.name==name and n.getRoleName()=='push button' and q.pyatspi.STATE_SHOWING in n.getState().getStates()),None)
def authenticate(name):
 q.wait_for(lambda:auth_button('Authenticate'),'real graphical authentication dialog',timeout=25)
 capture(name+'-prompt')
 field=q.wait_for(lambda:next((n for n in auth_nodes() if n.getRoleName()=='password text' and q.pyatspi.STATE_SHOWING in n.getState().getStates()),None),'password entry')
 assert field.queryComponent().grabFocus()
 q.run('xdotool','type','--clearmodifiers','--file',str(base/'.qa-password'))
 assert auth_button('Authenticate').queryAction().doAction(0)
 q.wait_for(lambda:not auth_button('Authenticate'),'authentication dialog dismissed',timeout=25)
def visible(text):return any(n.name==text and q.pyatspi.STATE_SHOWING in n.getState().getStates() for n in q.all_nodes(q.ottrin_app()))
try:
 subprocess.run(['dbus-update-activation-environment','--systemd','DISPLAY','XDG_SESSION_TYPE','XDG_CURRENT_DESKTOP','GTK_MODULES','NO_AT_BRIDGE'],check=True,capture_output=True)
 subprocess.run(['gsettings','set','org.gnome.desktop.interface','toolkit-accessibility','true'],check=True,capture_output=True)
 report['login_session_id']=os.environ.get('XDG_SESSION_ID')
 assert report['login_session_id'],'No PAM/logind session was established'
 wm=subprocess.Popen(['openbox'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL);children.append(wm)
 agentlog=(out/'authentication-agent.log').open('w')
 agent=subprocess.Popen(['/usr/lib/policykit-1-gnome/polkit-gnome-authentication-agent-1'],stdout=agentlog,stderr=agentlog);children.append(agent)
 time.sleep(1);assert agent.poll() is None,'Graphical authentication agent exited'
 image_input=base/'QA_APPIMAGE_INPUT.json'
 image_config=json.loads(image_input.read_text()) if image_input.exists() else None
 executable=base/image_config['name'] if image_config else next((base/'portable').glob('*/bin/ottrin'))
 fixture=base/'fixtures/preserve.txt';original=hashlib.sha256(fixture.read_bytes()).hexdigest()
 with (out/'app.log').open('w') as log:
  app=subprocess.Popen([str(executable),str(fixture.parent)],stdout=log,stderr=log);children.append(app);q.target_process_id=app.pid
  q.wait_for(q.ottrin_app,'owned Ottrin accessibility tree',timeout=30)
  if image_config:
   assert not os.environ.get('APPIMAGE_EXTRACT_AND_RUN')
   mapped=Path('/proc/'+str(app.pid)+'/exe').resolve()
   mount=next((line for line in Path('/proc/self/mountinfo').read_text().splitlines() if ' - fuse.' in line and str(mapped).startswith(line.split()[4]+'/')),None)
   assert mount,'Application is not running from a FUSE-mounted image'
   report['fuse_mount']={'executable':str(mapped),'mountinfo':mount}
  # The toolbar lock must provide the direct route to Services.
  q.click_accessible_name('Manage administrator helper','push button')
  q.wait_for(lambda:visible('Set up administrator helper'),'helper setup in Services')
  capture('setup-needed')
  q.click_accessible_name('Set up administrator helper','push button')
  cancel=q.wait_for(lambda:auth_button('Cancel'),'graphical cancel control',timeout=25)
  capture('cancel-prompt');assert cancel.queryAction().doAction(0)
  q.wait_for(lambda:visible('Helper setup was cancelled.'),'truthful setup cancellation',timeout=25)
  assert not Path('/usr/libexec/ottrin/ottrin-priv-helper').exists()
  capture('setup-cancelled');report['cases'].append('actual setup prompt cancelled; helper absent; cancellation visible')
  q.click_accessible_name('Set up administrator helper','push button');authenticate('setup')
  q.wait_for(lambda:visible('Update / reinstall helper'),'successful setup completion',timeout=45)
  helper=Path('/usr/libexec/ottrin/ottrin-priv-helper');policy=Path('/usr/share/polkit-1/actions/org.ottrin.filesystem.policy')
  assert helper.is_file() and helper.stat().st_uid==0 and helper.stat().st_mode&0o777==0o755
  assert policy.is_file() and policy.stat().st_uid==0
  capture('setup-ready');report['cases'].append('actual setup authenticated; root-owned helper and policy; ready UI')
  subprocess.run(['pkcheck','--revoke-temp'],check=True,capture_output=True)
  q.click_accessible_name('Update / reinstall helper','push button');authenticate('reinstall')
  q.wait_for(lambda:visible('Update / reinstall helper') and not visible('Working…'),'reinstall completion',timeout=45)
  assert helper.is_file() and helper.stat().st_uid==0
  capture('reinstall-ready');report['cases'].append('actual reinstall authenticated and completed')
  if image_config:
   settings=q.wait_for(lambda:q.run('xdotool','search','--onlyvisible','--name','^Ottrin Settings$',capture=True,check=False).stdout.strip(),'settings window')
   assert len(settings.split())==1
   q.run('xdotool','windowsize','--sync',settings,'1200','960')
   unit=base/'.config/systemd/user/ottrin-index.service'
   def service_state():
    pid=int(subprocess.check_output(['systemctl','--user','show','ottrin-index.service','--property=MainPID','--value'],text=True).strip() or 0)
    enabled=subprocess.run(['systemctl','--user','is-enabled','ottrin-index.service'],capture_output=True,text=True).stdout.strip()
    return {'pid':pid,'enabled':enabled,'installed':unit.exists()}
   def settled(enabled,running):
    value=service_state()
    return value if value['installed'] and (value['enabled']=='enabled')==enabled and (value['pid']>0)==running and not visible('Working…') else None
   report['service_states']=[]
   q.click_accessible_name('Enable automatic startup','push button')
   initial=q.wait_for(lambda:settled(True,True),'managed AppImage service installed',timeout=45)
   report['service_states'].append({'action':'install','state':initial})
   assert 'ottrin-index-appimage' in unit.read_text()
   for enabled in [False,True]:
    q.click_accessible_name('Start index automatically at sign-in','check box')
    state=q.wait_for(lambda:settled(enabled,True),'native startup checkbox persisted',timeout=30)
    assert state['pid']==initial['pid'],'Startup checkbox restarted the running daemon'
    report['service_states'].append({'action':'startup '+str(enabled)+' while running','state':state})
   capture('index-running-startup-on')
   q.click_accessible_name('Stop indexing','push button')
   q.wait_for(lambda:settled(True,False),'index stopped',timeout=30)
   for enabled in [False,True,False]:
    q.click_accessible_name('Start index automatically at sign-in','check box')
    state=q.wait_for(lambda:settled(enabled,False),'stopped service startup choice persisted',timeout=30)
    report['service_states'].append({'action':'startup '+str(enabled)+' while stopped','state':state})
   q.click_accessible_name('Update / reinstall service','push button')
   state=q.wait_for(lambda:settled(False,False),'repair preserves stopped disabled index',timeout=45)
   report['service_states'].append({'action':'repair disabled stopped service','state':state})
   capture('index-stopped-startup-off')
   q.click_accessible_name('Start / restart','push button')
   state=q.wait_for(lambda:settled(False,True),'explicit index restart',timeout=45)
   report['service_states'].append({'action':'explicit restart','state':state})
   q.click_accessible_name('Uninstall service','push button')
   q.wait_for(lambda:not unit.exists() and not visible('Working…'),'service uninstall',timeout=45)
   assert service_state()['pid']==0
   report['service_states'].append({'action':'uninstall','state':service_state()})
   capture('index-uninstalled')
  assert hashlib.sha256(fixture.read_bytes()).hexdigest()==original
  report['source_preserved']=True;report['status']='pass'
except BaseException as error:
 report.update(status='fail',error=repr(error))
 try:capture('failure')
 except Exception as failure:report['capture_error']=repr(failure)
finally:
 for child in reversed(children):
  if child.poll() is None:
   child.terminate()
   try:child.wait(timeout=10)
   except subprocess.TimeoutExpired:child.kill();child.wait(timeout=10)
 (out/'WORKER_RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report,indent=2));raise SystemExit(0 if report['status']=='pass' else 1)
