"""Installed browser Open/Save through a private portal and the exact delivered AppImage."""
from pathlib import Path
import argparse,importlib.util,hashlib,json,os,time,subprocess,secrets
from contextlib import ExitStack
r=Path.cwd();drivers=Path(__file__).resolve().parent;parser=argparse.ArgumentParser();parser.add_argument('--output',type=Path,required=True);parser.add_argument('--browser',choices=['firefox','falkon-flatpak'],required=True);args=parser.parse_args();e=args.output.resolve();assert e.is_relative_to(r/'.hermes/evidence');e.mkdir(exist_ok=True);out=e/'desktop';assert not out.exists()
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');dsk=importlib.util.module_from_spec(spec);spec.loader.exec_module(dsk)
image=r/'dist/packages/Ottrin-0.1.0-x86_64.AppImage';digest=hashlib.file_digest(image.open('rb'),'sha256').hexdigest();delivery=json.loads((r/'dist/packages/LINUX_PACKAGES_ACCEPTANCE.json').read_text());assert digest==delivery['artifacts'][image.name]
report={'status':'running','browser':args.browser,'scope':'Actual installed browser Open/Save and current AppImage chooser on private X11/bus/profile; private portal definition projection; production installation setup was tested separately','appimage_sha256':digest,'source_revision':delivery['source_revision']}
def prepare_portal(d):
 definitions=d.profile/'data/portal-definitions';definitions.mkdir();(definitions/'ottrin.portal').write_bytes((r/'packaging/linux/ottrin.portal').read_bytes())
 service=d.profile/'data/dbus-1/services/org.freedesktop.impl.portal.desktop.ottrin.service';service.parent.mkdir(parents=True);service.write_text('[D-BUS Service]\nName=org.freedesktop.impl.portal.desktop.ottrin\nExec="'+str(image)+'" portal\n')
 config=d.profile/'config/xdg-desktop-portal/portals.conf';config.parent.mkdir();config.write_text('[preferred]\ndefault=none\norg.freedesktop.impl.portal.FileChooser=ottrin\n');(definitions/'portals.conf').write_bytes(config.read_bytes());d.env.update(XDG_DESKTOP_PORTAL_DIR=str(definitions),GTK_USE_PORTAL='1',MOZ_ENABLE_WAYLAND='0',GDK_BACKEND='x11',XDG_SESSION_TYPE='x11')
try:
 with ExitStack() as stack:
  d=stack.enter_context(dsk.OwnedDesktop(out,prepare=prepare_portal))
  fixture=d.profile/'fixture';fixture.mkdir();source=fixture/'original.txt';payload=b'Ottrin Firefox file picker acceptance.\nOriginal bytes stay unchanged.\n';source.write_bytes(payload);dest=fixture/'saved';dest.mkdir()
  keys=['DISPLAY','DBUS_SESSION_BUS_ADDRESS','XDG_RUNTIME_DIR','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_CACHE_HOME','XDG_STATE_HOME','HOME','XDG_DESKTOP_PORTAL_DIR','WINIT_UNIX_BACKEND','LIBGL_ALWAYS_SOFTWARE','NO_AT_BRIDGE'];dsk.run(['dbus-update-activation-environment',*[k for k in keys if k in d.env]],d.env)
  d.spawn(['dbus-monitor','--session',"type='method_call',interface='org.freedesktop.impl.portal.FileChooser'"],'backend-requests.log')
  frontend=d.spawn(['/usr/libexec/xdg-desktop-portal','--replace','--verbose'],'frontend.log')
  profile=d.profile/'firefox';profile.mkdir();(profile/'user.js').write_text('\n'.join('user_pref('+json.dumps(k)+','+json.dumps(v)+');' for k,v in {'termsofuse.bypassNotification':True,'widget.use-xdg-desktop-portal.file-picker':1,'browser.shell.checkDefaultBrowser':False,'browser.startup.homepage_override.mstone':'ignore','browser.aboutwelcome.enabled':False,'datareporting.policy.dataSubmissionEnabled':False,'toolkit.telemetry.reportingpolicy.firstRun':False,'browser.newtabpage.enabled':False,'browser.startup.page':0,'app.update.auto':False}.items()))
  check=dsk.run(['xdpyinfo'],d.env);(out/'DISPLAY_CHECK.txt').write_text(check.stdout)
  (out/'ENVIRONMENT_KEYS.json').write_text(json.dumps({k:v for k,v in d.env.items() if k in ['DISPLAY','GDK_BACKEND','XAUTHORITY','WAYLAND_DISPLAY','XDG_SESSION_TYPE','XDG_RUNTIME_DIR','HOME','TMPDIR']},indent=2)+'\n')
  if args.browser=='firefox':
   host_bus=os.environ['DBUS_SESSION_BUS_ADDRESS'];host_env=os.environ.copy();assert host_bus.startswith('unix:path=/run/user/'+str(os.getuid())+'/bus')
   unit='app-firefox-ottrinqa'+secrets.token_hex(8)+'.scope';description='Owned Ottrin Firefox picker identity QA '+unit
   def manager(*args):return subprocess.run(['systemctl','--user',*args],env=host_env,text=True,capture_output=True,timeout=15)
   assert manager('show',unit,'--property=LoadState','--value').stdout.strip()=='not-found'
   def stop_scope():
    state=manager('show',unit,'--property=LoadState','--value').stdout.strip()
    if state!='not-found':
     assert manager('show',unit,'--property=Description','--value').stdout.strip()==description
     result=manager('stop',unit);assert result.returncode==0 or manager('show',unit,'--property=LoadState','--value').stdout.strip()=='not-found'
    report['owned_firefox_scope_cleanup']={'unit':unit,'load_state':manager('show',unit,'--property=LoadState','--value').stdout.strip()}
   stack.callback(stop_scope)
   browser_keys=keys+['PATH','LANG','LC_ALL','EGL_PLATFORM','GDK_BACKEND','XDG_SESSION_TYPE','GTK_USE_PORTAL','MOZ_ENABLE_WAYLAND']
   browser=d.spawn(['env','DBUS_SESSION_BUS_ADDRESS='+host_bus,'systemd-run','--user','--scope','--collect','--no-ask-password','--unit='+unit,'--description='+description,'--expand-environment=no','env','-i',*[key+'='+d.env[key] for key in browser_keys if key in d.env],'/usr/lib/firefox/firefox','--no-remote','--profile',str(profile),'about:blank'],'firefox.log')
   report['firefox_scope']={'unit':unit,'desktop_entry':'/usr/share/applications/firefox.desktop','purpose':'Correct real application attribution; inner Firefox retains only explicitly selected private session/environment values'}
  else:
   assert subprocess.run(['flatpak','info','org.kde.falkon'],capture_output=True).returncode==0
   private=d.profile/'browser';private.mkdir()
   for child in ['home','config','data','cache','state']:(private/child).mkdir()
   sandbox_env={key:str(private/value) for key,value in [('HOME','home'),('XDG_CONFIG_HOME','config'),('XDG_DATA_HOME','data'),('XDG_CACHE_HOME','cache'),('XDG_STATE_HOME','state')]}
   # The private portal returns temporary runtime URLs; Flatpak mounts the same
   # per-app document projection at /run/flatpak/doc. Alias only that projection
   # inside the sandbox; never grant the fixture or the host runtime directory.
   command=['flatpak','run','--nosocket=wayland','--socket=x11','--filesystem='+str(private),*["--env="+k+'='+v for k,v in sandbox_env.items()],'--env=QT_QPA_PLATFORM=xcb','--command=sh','org.kde.falkon','-c','test ! -e "$1" || exit 89; test -d /run/flatpak/doc || exit 90; mkdir -p "$2" && ln -s /run/flatpak/doc "$2/doc" || exit 91; cat /.flatpak-info; exec falkon about:blank','ottrin-sandbox-probe',str(source),str(d.profile/'runtime')]
   browser=d.spawn(command,'falkon.log')
   report['flatpak_scope']={'application':'org.kde.falkon','private_browser_data':str(private),'fixture_direct_filesystem_grant':False,'startup_guard':'Actual sandbox shell requires the outside original file to be invisible before invoking installed falkon; prints real /.flatpak-info','persistent_overrides_changed':False,'private_runtime_alias':'Sandbox-local symlink from the temporary runtime/doc URL prefix to Flatpak standard /run/flatpak/doc per-app mount; no fixture filesystem access added'}

  (out/'LIVE_SESSION.json').write_text(json.dumps({'profile':str(d.profile),'environment':{k:d.env[k] for k in ['DISPLAY','DBUS_SESSION_BUS_ADDRESS','HOME','XDG_RUNTIME_DIR']},'launcher_pid':browser.pid},indent=2)+'\n')
  def wait_windows(pattern):
   deadline=time.monotonic()+45
   while time.monotonic()<deadline:
    assert browser.poll() is None,(out/('firefox.log' if args.browser=='firefox' else 'falkon.log')).read_text()[-2000:]
    ids=dsk.run(['xdotool','search','--onlyvisible','--name',pattern],d.env,False).stdout.split()
    if ids:return ids
    if args.browser=='falkon-flatpak':
     wallets=dsk.run(['xdotool','search','--onlyvisible','--class','ksecretd'],d.env,False).stdout.split()
     for wallet in wallets:
      geometry=dict(line.split('=',1) for line in dsk.run(['xdotool','getwindowgeometry','--shell',wallet],d.env).stdout.splitlines() if '=' in line)
      if int(geometry['WIDTH'])<400 or int(geometry['HEIGHT'])<300:continue
      dsk.run(['import','-window',wallet,str(out/'private-wallet-declined.png')],d.env)
      dsk.run(['xdotool','windowactivate','--sync',wallet,'mousemove','--window',wallet,str(int(geometry['WIDTH'])-50),str(int(geometry['HEIGHT'])-28)],d.env);time.sleep(.15)
      dsk.run(['xdotool','mousedown','1'],d.env);time.sleep(.1);dsk.run(['xdotool','mouseup','1'],d.env)
      report['private_wallet_creation_declined']=True
    time.sleep(.1)
   inventory=dsk.run(['xwininfo','-root','-tree'],d.env,False);(out/'WINDOWS_AT_TIMEOUT.txt').write_text(inventory.stdout+inventory.stderr);dsk.run(['import','-window','root',str(out/'timeout.png')],d.env,False)
   raise RuntimeError('No owned window: '+pattern)
  window=wait_windows('Mozilla Firefox' if args.browser=='firefox' else 'Falkon')[-1]
  focus_deadline=time.monotonic()+10
  while True:
   dsk.run(['xdotool','windowactivate','--sync',window],d.env);time.sleep(.5)
   if dsk.run(['xdotool','getactivewindow'],d.env).stdout.strip()==window:break
   if time.monotonic()>=focus_deadline:
    (out/'WINDOWS_AT_FOCUS_TIMEOUT.txt').write_text(dsk.run(['xwininfo','-root','-tree'],d.env).stdout);dsk.run(['import','-window','root',str(out/'focus-timeout.png')],d.env);raise RuntimeError('Browser did not retain active focus')
  dsk.run(['import','-window',window,str(out/'before-open.png')],d.env);dsk.run(['xdotool','key','--clearmodifiers','ctrl+o'],d.env)
  chooser=wait_windows('Open')[-1];time.sleep(.3);dsk.run(['import','-window',chooser,str(out/'open.png')],d.env)
  chooser_pid=int(dsk.run(['xdotool','getwindowpid',chooser],d.env).stdout.strip());actual=Path(os.readlink('/proc/'+str(chooser_pid)+'/exe'));assert actual.name=='ottrin',actual
  report['chooser_executable']=str(actual);report['open_pid']=chooser_pid
  dsk.run(['xdotool','windowactivate','--sync',chooser,'key','ctrl+l','type','--clearmodifiers',str(fixture)],d.env);dsk.run(['xdotool','key','Return'],d.env);time.sleep(.4);dsk.run(['import','-window',chooser,str(out/'open-folder.png')],d.env)
  (out/'SESSION.json').write_text(json.dumps({'profile':str(d.profile),'fixture':str(fixture),'display':d.env['DISPLAY'],'environment':{k:d.env[k] for k in keys if k in d.env},'browser_window':window,'chooser_window':chooser,'chooser_pid':chooser_pid,'browser':args.browser,'payload_hex':payload.hex()},indent=2)+'\n')
  for stage in ['open','save']:
   subprocess.run(['python3',str(drivers/'control.py'),stage,'--output',str(e)],check=True)
  result=json.loads((e/'CONTROLLER_RESULT.json').read_text());assert result['status']=='pass';assert source.read_bytes()==payload;assert (dest/'original.txt').read_bytes()==payload;report.update(result)
  expected_id='firefox' if args.browser=='firefox' else 'org.kde.falkon'
  forwarded=(out/'backend-requests.log').read_text();assert forwarded.count('string "'+expected_id+'"')>=2 and 'member=OpenFile' in forwarded and 'member=SaveFile' in forwarded,forwarded[-4000:]
  if args.browser=='firefox':
   frontend_log=(out/'frontend.log').read_text();assert 'Assigning app ID "'+expected_id+'" to pid' in frontend_log,frontend_log[-3000:]
  report['actual_portal_app_id']=expected_id;report['identity_evidence']='Actual FileChooser backend OpenFile and SaveFile calls both carry the installed browser app ID';report['identity_limit']='Exact installed browser identity on private session; physical desktop and other browser packages remain separate.'
  if args.browser=='falkon-flatpak':
   sandbox_log=(out/'falkon.log').read_text();assert '[Application]' in sandbox_log and 'name=org.kde.falkon' in sandbox_log;report['real_flatpak_metadata_observed']=True
except BaseException as error:
 report.update(status='failed',error=repr(error));raise
finally:
 report['teardown']=json.loads((out/'TEARDOWN.json').read_text()) if (out/'TEARDOWN.json').exists() else None;(e/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
