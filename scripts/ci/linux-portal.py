#!/usr/bin/env python3
"""Production per-user portal lifecycle on a disposable GitHub Linux runner only."""
from pathlib import Path
import hashlib,json,os,signal,subprocess

assert (os.environ.get('GITHUB_ACTIONS')=='true' and os.environ.get('RUNNER_OS')=='Linux'
        and os.environ.get('RUNNER_ENVIRONMENT')=='github-hosted' and os.getuid()!=0), 'Disposable hosted Linux user required'
r=Path.cwd();out=r/'portal-evidence';out.mkdir(exist_ok=True)
binary=r/'target/debug/ottrin-app';home=Path.home()
config_home=Path(os.environ.get('XDG_CONFIG_HOME',str(home/'.config')))
data_home=Path(os.environ.get('XDG_DATA_HOME',str(home/'.local/share')))
state_home=Path(os.environ.get('XDG_STATE_HOME',str(home/'.local/state')))
config=config_home/'xdg-desktop-portal/ottrinci-portals.conf';receipt=state_home/'ottrin/portal-registration.json'
service=data_home/'dbus-1/services/org.freedesktop.impl.portal.desktop.ottrin.service';unit=config_home/'systemd/user/ottrin-portal.service'
original='[preferred]\ndefault=none\norg.freedesktop.impl.portal.FileChooser=none\n# preserve other provider preferences\n'
edit='# unrelated edit made after enabling\n';env=os.environ.copy();env['XDG_CURRENT_DESKTOP']='OttrinCI'
report={'scope':'Actual source CLI,protected installed definition,real user-manager/D-Bus activation and preference restoration on a disposable runner; no GUI or sandbox caller acceptance'}
def cancelled(*_):raise KeyboardInterrupt('Portal lifecycle cancelled')
for sig in [signal.SIGINT,signal.SIGTERM]:signal.signal(sig,cancelled)
def run(args,check=True,**kw):
 p=subprocess.run(args,env=env,capture_output=True,text=True,timeout=45,**kw)
 if check and p.returncode:raise RuntimeError({'command':args,'code':p.returncode,'stdout':p.stdout,'stderr':p.stderr})
 return p
def control(*args,check=True):return run(['systemctl','--user','--no-ask-password','--no-pager',*args],check)
def cli(*args):
 p=run([str(binary),'portal',*args]);report.setdefault('commands',[]).append({'arguments':args,'stdout':p.stdout,'stderr':p.stderr});return p.stdout
expected_definition=(r/'packaging/linux/ottrin.portal').read_bytes()
assert Path('/usr/share/xdg-desktop-portal/portals/ottrin.portal').read_bytes()==expected_definition
assert binary.is_file()
for path in [config,receipt,service,unit]:assert not path.exists() and not path.is_symlink(),f'Foreign state retained: {path}'
units=['xdg-desktop-portal.service','xdg-document-portal.service','xdg-permission-store.service']
active={name:control('is-active',name,check=False).returncode==0 for name in units}
manager=control('show-environment').stdout.splitlines();prior_desktop=next((line.split('=',1)[1] for line in manager if line.startswith('XDG_CURRENT_DESKTOP=')),None)
created=[];allocated=False;environment_changed=False
try:
 # Record ownership before allocating paths or changing the disposable manager.
 (out/'OWNER.json').write_text(json.dumps({'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'config':str(config),'managed_paths':[str(p) for p in [receipt,service,unit]],'units_before':active,'manager_desktop_before':prior_desktop},indent=2)+'\n')
 for parent in [config.parent,service.parent,unit.parent,receipt.parent]:
  for candidate in reversed([parent,*parent.parents]):
   if not candidate.exists():candidate.mkdir(mode=0o700);created.append(candidate)
 config.write_text(original);allocated=True
 environment_changed=True
 run(['dbus-update-activation-environment','--systemd','XDG_CURRENT_DESKTOP'])
 assert not json.loads(cli('status','--json'))['enabled']
 cli('enable');pending=json.loads(cli('status','--json'));assert pending['enabled'] and pending['pending'] and not pending['ready'],pending
 cli('apply');ready=json.loads(cli('status','--json'));assert ready['enabled'] and ready['ready'] and not ready['pending'],ready
 assert control('is-active','ottrin-portal.service').returncode==0
 pid=int(control('show','ottrin-portal.service','--property=MainPID','--value').stdout.strip());assert pid>0
 assert Path(f'/proc/{pid}/comm').read_text().strip()=='ottrin-portal'
 config.write_text(config.read_text()+edit)
 cli('enable');cli('apply');assert json.loads(cli('status','--json'))['ready']
 cli('disable');assert not service.exists() and not unit.exists();assert receipt.exists()
 restored=config.read_text();assert 'org.freedesktop.impl.portal.FileChooser=none' in restored and edit in restored and 'default=none' in restored
 cli('apply');off=json.loads(cli('status','--json'));assert not off['enabled'] and not off['ready'] and not off['pending'],off
 assert not receipt.exists();assert control('is-active','ottrin-portal.service',check=False).returncode!=0
 report.update(status='pass',ready=ready,off=off,broker_pid=pid,prior_chooser_restored=True,unrelated_edit_preserved=True,managed_activation_removed=True)
finally:
 try:
  if allocated:
   if receipt.exists():cli('disable');cli('apply')
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
  report['teardown']={'owned_files_removed':all(not p.exists() for p in [config,receipt,service,unit]),'original_unit_active_states_restored':True,'manager_desktop_restored':True}
 except BaseException as error:
  report['cleanup_error']=repr(error);report['status']='failed cleanup';raise
 finally:(out/'ACCEPTANCE.json').write_text(json.dumps(report,indent=2)+'\n')
