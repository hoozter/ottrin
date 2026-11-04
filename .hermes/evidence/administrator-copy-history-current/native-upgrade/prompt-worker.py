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
  q.wait_for(lambda:visible('Update needed'),'incompatible installed-helper status')
  guidance='Administrator helper needs an update. Open Services and choose Update / reinstall helper.'
  assert visible(guidance),'Updated helper guidance is missing or truncated in accessibility tree'
  report['update_guidance']=guidance
  assert visible('Update / reinstall helper') and not visible('Set up administrator helper') and not visible('Installed and ready')
  helper=Path('/usr/libexec/ottrin/ottrin-priv-helper')
  old_config=json.loads((base/'QA_OLD_HELPER_INPUT.json').read_text())
  assert hashlib.sha256(helper.read_bytes()).hexdigest()==old_config['sha256']
  capture('old-helper-update-needed')
  q.click_accessible_name('Update / reinstall helper','push button')
  cancel=q.wait_for(lambda:auth_button('Cancel'),'graphical update cancel control',timeout=25)
  capture('update-cancel-prompt');assert cancel.queryAction().doAction(0)
  q.wait_for(lambda:visible('Helper setup was cancelled.'),'truthful update cancellation',timeout=25)
  assert hashlib.sha256(helper.read_bytes()).hexdigest()==old_config['sha256']
  assert visible('Update needed') and not visible('Installed and ready')
  capture('update-cancelled');report['cases'].append('old installed helper shows Update needed; cancel preserves exact old helper and unavailable status')
  q.click_accessible_name('Update / reinstall helper','push button');authenticate('update')
  q.wait_for(lambda:visible('Installed and ready'),'successful authenticated helper update',timeout=45)
  assert helper.is_file() and helper.stat().st_uid==0 and helper.stat().st_mode&0o777==0o755
  assert hashlib.sha256(helper.read_bytes()).hexdigest()!=old_config['sha256']
  assert subprocess.run([str(helper),'--check-protocol','2','--copy-recovery'],capture_output=True).returncode==0
  policy=Path('/usr/share/polkit-1/actions/org.ottrin.filesystem.policy')
  assert policy.is_file() and policy.stat().st_uid==0
  report['updated_helper_sha256']=hashlib.sha256(helper.read_bytes()).hexdigest()
  capture('updated-helper-ready');report['cases'].append('authenticated update replaces old helper with protocol-compatible root-owned helper; actual Ready status')
  subprocess.run(['pkcheck','--revoke-temp'],check=True,capture_output=True)
  q.click_accessible_name('Update / reinstall helper','push button');authenticate('reinstall')
  q.wait_for(lambda:visible('Update / reinstall helper') and not visible('Working…'),'reinstall completion',timeout=45)
  assert helper.is_file() and helper.stat().st_uid==0
  capture('reinstall-ready');report['cases'].append('actual reinstall authenticated and completed')
  if (base/'QA_PROTECTED_INPUT.json').exists():
   report['session_properties']=subprocess.check_output(['loginctl','show-session',report['login_session_id'],'-p','Active','-p','Remote','-p','Class','-p','Seat','-p','Type','-p','Service','-p','State'],text=True)
   report['helper_policy']=subprocess.check_output(['pkaction','--action-id','org.ottrin.filesystem.privileged','--verbose'],text=True)
   session=dict(line.split('=',1) for line in report['session_properties'].splitlines() if '=' in line)
   assert session.get('Seat') and session.get('Active')=='yes','Positive protected-file acceptance requires a genuine active local-seat fixture; do not relax policy or mistake background denial for cancellation'
   destination=Path('/var/lib/ottrin-owned-protected-qa')
   try:
    (destination/'ordinary-write.txt').write_text('Must be refused')
    raise AssertionError('Ordinary account unexpectedly wrote the protected directory')
   except PermissionError:pass
   def copy_request(source):return json.dumps({'protocol_version':2,'command':{'File':{'Copy':{'sources':[str(source)],'destination':str(destination),'conflict':'Skip'}}},'context':{'reason':'Owned native polkit acceptance','cwd':str(fixture.parent)}}).encode()
   def elevated_copy(source):
    child=subprocess.Popen(['pkexec','/usr/libexec/ottrin/ottrin-priv-helper','--stdio-json'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
    children.append(child);child.stdin.write(copy_request(source));child.stdin.close();return child
   subprocess.run(['pkcheck','--revoke-temp'],check=True,capture_output=True)
   report['temporary_authorizations_after_revoke']=subprocess.run(['pkcheck','--list-temp'],capture_output=True,text=True).stdout
   cancelled=elevated_copy(fixture.parent/'cancel-copy.txt')
   try:
    cancel=q.wait_for(lambda:auth_button('Cancel'),'protected-operation cancel prompt',timeout=25)
   except BaseException:
    report['protected_cancel_process']={'exit':cancelled.poll(),'stdout':cancelled.stdout.read().decode() if cancelled.poll() is not None else None,'stderr':cancelled.stderr.read().decode() if cancelled.poll() is not None else None}
    raise
   capture('protected-copy-cancel-prompt');assert cancel.queryAction().doAction(0)
   assert cancelled.wait(timeout=15)==126
   approved=elevated_copy(fixture);authenticate('protected-copy')
   assert approved.wait(timeout=20)==0,approved.stderr.read().decode()
   response=json.loads(approved.stdout.read());assert response['status']=='Success',response
   def atomic_report(path,value):
    temporary=path.with_suffix('.next');temporary.write_text(json.dumps(value));temporary.replace(path)
   def root_verify(phase):
    atomic_report(out/(phase+'_READY.json'),{'status':'awaiting root byte/ownership verification'})
    proof=q.wait_for(lambda:json.loads((out/(phase+'_VERIFIED.json')).read_text()) if (out/(phase+'_VERIFIED.json')).exists() else None,'independent root Copy verification',timeout=20)
    assert proof['root_owned'] and proof['exact_source_bytes'];return proof
   def elevated_command(command):
    child=subprocess.Popen(['pkexec','/usr/libexec/ottrin/ottrin-priv-helper','--stdio-json'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
    children.append(child);child.stdin.write(json.dumps({'protocol_version':2,'command':command,'context':{'reason':'Owned native Copy recovery','cwd':str(fixture.parent)}}).encode());child.stdin.close();return child
   references=response['payload']['Copy'];assert len(references['transactions'])==len(references['paths'])==references['completed_sources']==1
   report['helper_copy_root_verification']=root_verify('HELPER_COPY')
   subprocess.run(['pkcheck','--revoke-temp'],check=True,capture_output=True)
   undo=elevated_command({'UndoCopy':{'transactions':references['transactions']}});authenticate('protected-undo')
   assert undo.wait(timeout=20)==0,undo.stderr.read().decode()
   assert json.loads(undo.stdout.read())['status']=='Success'
   subprocess.run(['pkcheck','--revoke-temp'],check=True,capture_output=True)
   cleanup=elevated_command({'CleanupCopy':{'transactions':references['transactions']}});authenticate('protected-cleanup')
   assert cleanup.wait(timeout=20)==0,cleanup.stderr.read().decode()
   assert json.loads(cleanup.stdout.read())['status']=='Success'
   report['protected_helper']={'ordinary_write_denied':True,'cancel_exit':126,'authenticated_response':response,'undo_and_cleanup':'Success through fresh authenticated helper processes'}
   capture('protected-helper-cycle-completed')
   # Real main-window denied operation -> administrator retry -> Activity Undo/cleanup.
   settings=q.wait_for(lambda:q.run('xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','^Ottrin Settings$',capture=True,check=False).stdout.strip(),'owned Settings window')
   assert len(settings.split())==1,{'selector':'owned Settings','matching_windows':settings}
   q.xkey(settings,'alt+F4')
   main=q.wait_for(lambda:q.run('xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','^Ottrin$',capture=True,check=False).stdout.strip(),'owned main window')
   assert len(main.split())==1,{'selector':'owned main','matching_windows':main}
   q.xkey(main,'Escape')
   q.run('xdotool','type','--clearmodifiers','c')
   q.wait_for(lambda:next((n for n in q.all_nodes(q.ottrin_app()) if n.getRoleName() in {'entry','text'} and q.pyatspi.STATE_FOCUSED in n.getState().getStates()),None),'focused command input')
   q.run('xdotool','type','--clearmodifiers','--delay','5','p '+str(fixture)+' '+str(destination))
   q.xkey(main,'Return')
   q.wait_for(lambda:visible('Retry As Administrator…'),'denied Copy administrator retry',timeout=30)
   capture('gui-copy-denied')
   subprocess.run(['pkcheck','--revoke-temp'],check=True,capture_output=True)
   q.click_accessible_name('Retry As Administrator…','push button');authenticate('gui-copy')
   q.wait_for(lambda:visible('Operation completed with administrator privileges.'),'successful actual app Copy retry',timeout=30)
   report['gui_copy_root_verification']=root_verify('GUI_COPY')
   report['activity_controls']=[{'name':n.name,'role':n.getRoleName()} for n in q.all_nodes(q.ottrin_app()) if 'Activity' in n.name]
   (out/'before-activity-a11y.txt').write_text('\n'.join(q.visible_a11y_nodes()))
   q.click_accessible_name('Show or hide Activity')
   q.wait_for(lambda:visible('Copy with administrator privileges'),'elevated Copy Activity ownership',timeout=20)
   q.wait_for(lambda:visible('Undo copy'),'actual app Undo control',timeout=20)
   capture('gui-copy-activity')
   # A normal GUI exit loses every in-memory elevated receipt. Root journals
   # remain the only authority for the explicit authenticated rediscovery.
   q.xkey(main,'alt+F4');app.wait(timeout=30)
   app=subprocess.Popen([str(executable),str(fixture.parent)],stdout=log,stderr=log)
   children.append(app);q.target_process_id=app.pid
   q.wait_for(q.ottrin_app,'restarted owned Ottrin accessibility tree',timeout=30)
   if not visible('Load administrator Copy history'):
    q.click_accessible_name('Show or hide Activity')
   q.wait_for(lambda:visible('Load administrator Copy history'),'opt-in history control',timeout=20)
   assert not visible('Administrator Copy from previous session')
   subprocess.run(['pkcheck','--revoke-temp'],check=True,capture_output=True)
   q.click_accessible_name('Load administrator Copy history','push button')
   cancel=q.wait_for(lambda:auth_button('Cancel'),'history authentication cancellation')
   capture('history-cancel-prompt');assert cancel.queryAction().doAction(0)
   q.wait_for(lambda:visible('Load administrator Copy history'),'cancelled history scan returns control',timeout=30)
   assert not visible('Administrator Copy from previous session')
   capture('history-cancelled')
   q.click_accessible_name('Load administrator Copy history','push button');authenticate('history-load')
   q.wait_for(lambda:visible('Administrator Copy from previous session'),'canonical history restored after GUI restart',timeout=30)
   q.wait_for(lambda:visible('Undo copy'),'restored Undo control',timeout=20)
   capture('history-restored')
   report['history_restart']={'normal_gui_close':True,'new_process':True,'cancel_preserved_recovery':True,'authenticated_discovery':True}
   subprocess.run(['pkcheck','--revoke-temp'],check=True,capture_output=True)
   q.click_accessible_name('Undo copy','push button');authenticate('gui-undo')
   q.wait_for(lambda:visible('Clear retained Copy data…'),'completed app Undo retains cleanup ownership',timeout=30)
   capture('gui-undo-completed')
   q.click_accessible_name('Clear retained Copy data…','push button')
   q.wait_for(lambda:visible('Clear retained data'),'explicit retained-data confirmation')
   subprocess.run(['pkcheck','--revoke-temp'],check=True,capture_output=True)
   q.click_accessible_name('Clear retained data','push button');authenticate('gui-cleanup')
   q.wait_for(lambda:not visible('Administrator Copy from previous session'),'completed Copy cleanup removes its restored Activity row',timeout=30)
   capture('gui-copy-cleanup-completed')
   report['gui_privileged_copy']='Actual cp denied, authenticated retry copied exact root-owned bytes; normal GUI restart lost live receipts; explicit history cancellation preserved journals, authenticated discovery restored Activity; fresh authenticated Undo and confirmed cleanup completed'
  assert hashlib.sha256(fixture.read_bytes()).hexdigest()==original
  report['source_preserved']=True;report['status']='pass'
except BaseException as error:
 import traceback
 report['traceback']=traceback.format_exc()
 try:(out/'failure-a11y.txt').write_text('\n'.join(q.visible_a11y_nodes()))
 except Exception as failure:report['a11y_dump_error']=repr(failure)
 report.update(status='fail',error=repr(error))
 try:capture('failure')
 except Exception as failure:report['capture_error']=repr(failure)
finally:
 for child in reversed(children):
  if child.poll() is None:
   child.terminate()
   try:child.wait(timeout=10)
   except subprocess.TimeoutExpired:child.kill();child.wait(timeout=10)
 temporary=out/'WORKER_RESULT.next';temporary.write_text(json.dumps(report,indent=2)+'\n');temporary.replace(out/'WORKER_RESULT.json')
print(json.dumps(report,indent=2));raise SystemExit(0 if report['status']=='pass' else 1)
