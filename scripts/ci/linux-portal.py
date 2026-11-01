#!/usr/bin/env python3
"""Production per-user portal lifecycle on a disposable GitHub Linux runner only."""
from pathlib import Path
import hashlib,json,os,re,signal,subprocess,time

assert (os.environ.get('GITHUB_ACTIONS')=='true' and os.environ.get('RUNNER_OS')=='Linux'
        and os.environ.get('RUNNER_ENVIRONMENT')=='github-hosted' and os.getuid()!=0), 'Disposable hosted Linux user required'
r=Path.cwd();out=r/'portal-evidence';out.mkdir(exist_ok=True)
binary=r/'target/debug/ottrin-app';home=Path.home()
config_home=Path(os.environ.get('XDG_CONFIG_HOME',str(home/'.config')))
data_home=Path(os.environ.get('XDG_DATA_HOME',str(home/'.local/share')))
state_home=Path(os.environ.get('XDG_STATE_HOME',str(home/'.local/state')))
config=config_home/'xdg-desktop-portal/ottrinci-portals.conf';receipt=state_home/'ottrin/portal-registration.json'
service=data_home/'dbus-1/services/org.freedesktop.impl.portal.desktop.ottrin.service';unit=config_home/'systemd/user/ottrin-portal.service'
folder_config=config_home/'ottrinci-mimeapps.list';folder_receipt=state_home/'ottrin/folder-registration.json'
folder_service=data_home/'dbus-1/services/org.freedesktop.FileManager1.service'
folder_entry=data_home/'applications/org.ottrin.Ottrin.Folders.desktop';folder_icon=data_home/'ottrin/integration/icon.png';previous_entry=data_home/'applications/org.ottrin.Previous.CI.desktop'
folder_original='[Default Applications]\ninode/directory=org.ottrin.Previous.CI.desktop;\napplication/pdf=reader.desktop;\n'
previous_contents='[Desktop Entry]\nType=Application\nName=Previous CI directory handler\nExec=/usr/bin/true %f\nNoDisplay=true\nMimeType=inode/directory;\n'
original='[preferred]\ndefault=none\norg.freedesktop.impl.portal.FileChooser=none\n# preserve other provider preferences\n'
edit='# unrelated edit made after enabling\n';env=os.environ.copy();env['XDG_CURRENT_DESKTOP']='OttrinCI';env['LC_ALL']='C'
report={'scope':'Actual source CLI,protected installed definition,real user-manager/D-Bus activation and preference restoration on a disposable runner; no GUI or sandbox caller acceptance'}
def cancelled(*_):raise KeyboardInterrupt('Portal lifecycle cancelled')
for sig in [signal.SIGINT,signal.SIGTERM]:signal.signal(sig,cancelled)
def run(args,check=True,**kw):
 p=subprocess.run(args,env=env,capture_output=True,text=True,timeout=45,**kw)
 if check and p.returncode:raise RuntimeError({'command':args,'code':p.returncode,'stdout':p.stdout,'stderr':p.stderr})
 return p
def control(*args,check=True):return run(['systemctl','--user','--no-ask-password','--no-pager',*args],check)
def manage(integration,*args):
 p=run([str(binary),integration,*args]);report.setdefault('commands',[]).append({'integration':integration,'arguments':args,'stdout':p.stdout,'stderr':p.stderr});return p.stdout
def wait_start_window():
 # The complete lifecycle deliberately restarts the real frontend more often
 # than a normal setup. Respect its rate limiter between independent scenarios.
 interval=control('show','xdg-desktop-portal.service','--property=StartLimitIntervalUSec','--value').stdout.strip()
 parsed=run(['systemd-analyze','timespan',interval]).stdout
 match=re.search(r'^\s*us:\s*(\d+)\s*$',parsed,re.MULTILINE)
 assert match,parsed
 seconds=int(match.group(1))/1_000_000+0.1
 assert 0<seconds<=60, f'Unexpected hosted start-limit interval: {interval}'
 report.setdefault('start_limit_waits_seconds',[]).append(seconds)
 time.sleep(seconds)
expected_definition=(r/'packaging/linux/ottrin.portal').read_bytes()
assert Path('/usr/share/xdg-desktop-portal/portals/ottrin.portal').read_bytes()==expected_definition
assert binary.is_file()
for path in [config,receipt,service,unit,folder_config,folder_receipt,folder_entry,folder_icon,folder_service,previous_entry]:assert not path.exists() and not path.is_symlink(),f'Foreign state retained: {path}'
units=['xdg-desktop-portal.service','xdg-document-portal.service','xdg-permission-store.service']
active={name:control('is-active',name,check=False).returncode==0 for name in units}
manager=control('show-environment').stdout.splitlines();prior_desktop=next((line.split('=',1)[1] for line in manager if line.startswith('XDG_CURRENT_DESKTOP=')),None)
created=[];allocated=False;folder_allocated=False;environment_changed=False
try:
 # Record ownership before allocating paths or changing the disposable manager.
 (out/'OWNER.json').write_text(json.dumps({'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'config':str(config),'managed_paths':[str(p) for p in [receipt,service,unit,folder_receipt,folder_entry,folder_icon,folder_service]],'folder_fixture_paths':[str(p) for p in [folder_config,previous_entry]],'units_before':active,'manager_desktop_before':prior_desktop},indent=2)+'\n')
 for parent in [config.parent,service.parent,unit.parent,receipt.parent,folder_entry.parent,folder_icon.parent]:
  for candidate in reversed([parent,*parent.parents]):
   if not candidate.exists():candidate.mkdir(mode=0o700);created.append(candidate)
 config.write_text(original);allocated=True
 environment_changed=True
 run(['dbus-update-activation-environment','--systemd','XDG_CURRENT_DESKTOP'])
 assert not json.loads(manage('portal','status','--json'))['enabled']
 manage('portal','enable');pending=json.loads(manage('portal','status','--json'));assert pending['enabled'] and pending['pending'] and not pending['ready'],pending
 manage('portal','apply');ready=json.loads(manage('portal','status','--json'));assert ready['enabled'] and ready['ready'] and not ready['pending'],ready
 assert control('is-active','ottrin-portal.service').returncode==0
 pid=int(control('show','ottrin-portal.service','--property=MainPID','--value').stdout.strip());assert pid>0
 assert Path(f'/proc/{pid}/comm').read_text().strip()=='ottrin-portal'
 config.write_text(config.read_text()+edit)
 manage('portal','enable');manage('portal','apply');assert json.loads(manage('portal','status','--json'))['ready']
 manage('portal','disable');assert not service.exists() and not unit.exists();assert receipt.exists()
 restored=config.read_text();assert 'org.freedesktop.impl.portal.FileChooser=none' in restored and edit in restored and 'default=none' in restored
 manage('portal','apply');off=json.loads(manage('portal','status','--json'));assert not off['enabled'] and not off['ready'] and not off['pending'],off
 assert not receipt.exists();assert control('is-active','ottrin-portal.service',check=False).returncode!=0
 folder_allocated=True;folder_config.write_text(folder_original);previous_entry.write_text(previous_contents)
 def default_folder():return run(['/usr/bin/python3','-c',"from gi.repository import Gio; a=Gio.AppInfo.get_default_for_type('inode/directory',False);print(a.get_id() if a else '')"]).stdout.strip()
 assert default_folder()=='org.ottrin.Previous.CI.desktop'
 manage('folders','enable');folder_ready=json.loads(manage('folders','status','--json'));assert folder_ready['enabled'] and folder_ready['ready'],folder_ready
 assert default_folder()=='org.ottrin.Ottrin.Folders.desktop'
 folder_config.write_text(folder_config.read_text().replace('reader.desktop','new-reader.desktop'))
 manage('folders','enable');manage('folders','disable');assert default_folder()=='org.ottrin.Previous.CI.desktop'
 assert 'application/pdf=new-reader.desktop;' in folder_config.read_text();assert previous_entry.read_text()==previous_contents
 assert not any(path.exists() for path in [folder_receipt,folder_entry,folder_icon,folder_service])
 report['folders']={'ready':folder_ready,'native_gio_default_verified':True,'previous_default_restored':True,'unrelated_edit_preserved':True,'foreign_entry_preserved':True,'owned_assets_removed':True}
 # Exercise application removal through the production manager, not only standalone controls.
 manage('portal','enable');manage('portal','apply');manage('folders','enable')
 cleanup=run([str(binary),'--uninstall']);report['application_cleanup']={'stdout':cleanup.stdout,'stderr':cleanup.stderr}
 assert not any(path.exists() for path in [receipt,service,unit,folder_receipt,folder_entry,folder_icon,folder_service])
 assert config.read_text()==original+edit
 assert default_folder()=='org.ottrin.Previous.CI.desktop'
 assert 'application/pdf=new-reader.desktop;' in folder_config.read_text()
 assert previous_entry.read_text()==previous_contents
 wait_start_window()
 # A previously disabled, pending portal must apply removal without stopping a missing unit again.
 manage('portal','enable');manage('portal','apply');manage('portal','disable')
 cleanup=run([str(binary),'--uninstall']);report['pending_portal_cleanup']={'stdout':cleanup.stdout,'stderr':cleanup.stderr}
 assert not any(path.exists() for path in [receipt,service,unit])
 assert config.read_text()==original+edit
 report.update(status='pass',ready=ready,off=off,broker_pid=pid,prior_chooser_restored=True,unrelated_edit_preserved=True,managed_activation_removed=True)
except BaseException as error:
 report.update(status='failed',error=repr(error));raise
finally:
 try:
  if folder_allocated:
   if folder_receipt.exists():manage('folders','disable')
   for path in [folder_receipt,folder_entry,folder_icon,folder_service]:assert not path.exists(),f'Owned folder setup remains: {path}'
   assert folder_config.read_text() in [folder_original,folder_original.replace('reader.desktop','new-reader.desktop')], 'Changed folder fixture retained'
   assert previous_entry.read_text()==previous_contents
   folder_config.unlink();previous_entry.unlink()
  if allocated:
   if receipt.exists():
    wait_start_window()
    if json.loads(manage('portal','status','--json'))['enabled']:manage('portal','disable')
    manage('portal','apply')
   for path in [receipt,service,unit]:assert not path.exists(),f'Owned setup remains: {path}'
   assert config.read_text() in [original,original+edit], 'Changed fixture configuration retained'
   config.unlink()
  if environment_changed:
   restore=env.copy();restore['XDG_CURRENT_DESKTOP']=prior_desktop or ''
   subprocess.run(['dbus-update-activation-environment','--systemd','XDG_CURRENT_DESKTOP'],env=restore,check=True,capture_output=True,timeout=10)
   if prior_desktop is None:control('unset-environment','XDG_CURRENT_DESKTOP')
  for name,was_active in active.items():
   control('restart' if was_active else 'stop',name)
  for directory in reversed(created):
   if directory.exists() and not any(directory.iterdir()):directory.rmdir()
  report['teardown']={'owned_files_removed':all(not p.exists() for p in [config,receipt,service,unit,folder_config,folder_receipt,folder_entry,folder_icon,folder_service,previous_entry]),'original_unit_active_states_restored':True,'manager_desktop_restored':True}
 except BaseException as error:
  report['cleanup_error']=repr(error);report['status']='failed cleanup';raise
 finally:(out/'ACCEPTANCE.json').write_text(json.dumps(report,indent=2)+'\n')
