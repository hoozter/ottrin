"""Real Firefox Open/Save through a private portal and the exact delivered AppImage."""
from pathlib import Path
import importlib.util,hashlib,json,os,time,subprocess,secrets
from contextlib import ExitStack
r=Path.cwd();e=Path(__file__).resolve().parent;out=e/'desktop';assert not out.exists()
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');dsk=importlib.util.module_from_spec(spec);spec.loader.exec_module(dsk)
image=r/'dist/packages/Ottrin-0.1.0-x86_64.AppImage';digest=hashlib.file_digest(image.open('rb'),'sha256').hexdigest();assert digest=='617c03c396ea3365924d82ae32edd898cc70f24fa99fd2b4daa59d795f583ad4'
report={'status':'running','scope':'Actual Firefox Open/Save and current AppImage chooser on private X11/bus/profile; private portal definition projection; production installation setup was tested separately','appimage_sha256':digest}
try:
 with ExitStack() as stack:
  d=stack.enter_context(dsk.OwnedDesktop(out))
  fixture=d.profile/'fixture';fixture.mkdir();source=fixture/'original.txt';payload=b'Ottrin Firefox file picker acceptance.\nOriginal bytes stay unchanged.\n';source.write_bytes(payload);dest=fixture/'saved';dest.mkdir()
  definitions=d.profile/'data/portal-definitions';definitions.mkdir();(definitions/'ottrin.portal').write_bytes((r/'packaging/linux/ottrin.portal').read_bytes())
  service=d.profile/'data/dbus-1/services/org.freedesktop.impl.portal.desktop.ottrin.service';service.parent.mkdir(parents=True);service.write_text('[D-BUS Service]\nName=org.freedesktop.impl.portal.desktop.ottrin\nExec="'+str(image)+'" portal\n')
  config=d.profile/'config/xdg-desktop-portal/portals.conf';config.parent.mkdir();config.write_text('[preferred]\ndefault=none\norg.freedesktop.impl.portal.FileChooser=ottrin\n');(definitions/'portals.conf').write_bytes(config.read_bytes());d.env.update(XDG_DESKTOP_PORTAL_DIR=str(definitions),GTK_USE_PORTAL='1',MOZ_ENABLE_WAYLAND='0',GDK_BACKEND='x11',XDG_SESSION_TYPE='x11')
  bus=d.spawn(['dbus-daemon','--session','--nofork','--print-address=1'],'portal-bus.log',pipe=True);d.env['DBUS_SESSION_BUS_ADDRESS']=d.ready_line(bus)
  keys=['DISPLAY','DBUS_SESSION_BUS_ADDRESS','XDG_RUNTIME_DIR','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_CACHE_HOME','XDG_STATE_HOME','HOME','XDG_DESKTOP_PORTAL_DIR','WINIT_UNIX_BACKEND','LIBGL_ALWAYS_SOFTWARE','NO_AT_BRIDGE'];dsk.run(['dbus-update-activation-environment',*[k for k in keys if k in d.env]],d.env)
  frontend=d.spawn(['/usr/libexec/xdg-desktop-portal','--replace','--verbose'],'frontend.log')
  profile=d.profile/'firefox';profile.mkdir();(profile/'user.js').write_text('\n'.join('user_pref('+json.dumps(k)+','+json.dumps(v)+');' for k,v in {'termsofuse.bypassNotification':True,'widget.use-xdg-desktop-portal.file-picker':1,'browser.shell.checkDefaultBrowser':False,'browser.startup.homepage_override.mstone':'ignore','browser.aboutwelcome.enabled':False,'datareporting.policy.dataSubmissionEnabled':False,'toolkit.telemetry.reportingpolicy.firstRun':False,'browser.newtabpage.enabled':False,'browser.startup.page':0,'app.update.auto':False}.items()))
  check=dsk.run(['xdpyinfo'],d.env);(out/'DISPLAY_CHECK.txt').write_text(check.stdout)
  (out/'ENVIRONMENT_KEYS.json').write_text(json.dumps({k:v for k,v in d.env.items() if k in ['DISPLAY','GDK_BACKEND','XAUTHORITY','WAYLAND_DISPLAY','XDG_SESSION_TYPE','XDG_RUNTIME_DIR','HOME','TMPDIR']},indent=2)+'\n')
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

  def wait_windows(pattern):
   deadline=time.monotonic()+45
   while time.monotonic()<deadline:
    assert browser.poll() is None,(out/'firefox.log').read_text()[-2000:]
    ids=dsk.run(['xdotool','search','--onlyvisible','--name',pattern],d.env,False).stdout.split()
    if ids:return ids
    time.sleep(.1)
   raise RuntimeError('No owned window: '+pattern)
  window=wait_windows('Mozilla Firefox')[-1];dsk.run(['xdotool','windowactivate','--sync',window,'key','ctrl+o'],d.env)
  chooser=wait_windows('Open File')[-1];time.sleep(.3);dsk.run(['import','-window',chooser,str(out/'open.png')],d.env)
  chooser_pid=int(dsk.run(['xdotool','getwindowpid',chooser],d.env).stdout.strip());actual=Path(os.readlink('/proc/'+str(chooser_pid)+'/exe'));assert actual.name=='ottrin',actual
  report['chooser_executable']=str(actual);report['open_pid']=chooser_pid
  dsk.run(['xdotool','windowactivate','--sync',chooser,'key','ctrl+l','type','--clearmodifiers',str(fixture)],d.env);dsk.run(['xdotool','key','Return'],d.env);time.sleep(.4);dsk.run(['import','-window',chooser,str(out/'open-folder.png')],d.env)
  (out/'SESSION.json').write_text(json.dumps({'profile':str(d.profile),'fixture':str(fixture),'display':d.env['DISPLAY'],'environment':{k:d.env[k] for k in keys if k in d.env},'browser_window':window,'chooser_window':chooser,'chooser_pid':chooser_pid},indent=2)+'\n')
  for stage in ['open','save']:
   subprocess.run(['python3',str(e/'control.py'),stage],check=True)
  result=json.loads((e/'CONTROLLER_RESULT.json').read_text());assert result['status']=='pass';assert source.read_bytes()==payload;assert (dest/'original.txt').read_bytes()==payload;report.update(result)
  frontend_log=(out/'frontend.log').read_text();assert 'Assigning app ID \"firefox\" to pid' in frontend_log,frontend_log[-3000:];report['actual_portal_app_id']='firefox';report['identity_limit']='Ordinary installed Firefox identity accepted through its desktop-entry systemd scope; this is not installed Flatpak-browser acceptance.'
except BaseException as error:
 report.update(status='failed',error=repr(error));raise
finally:
 report['teardown']=json.loads((out/'TEARDOWN.json').read_text()) if (out/'TEARDOWN.json').exists() else None;(e/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
