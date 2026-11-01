"""Real Flatpak build sandbox -> real private portal frontend -> Ottrin -> FUSE export."""
from pathlib import Path
import importlib.util,json,hashlib,time,os,subprocess
r=Path.cwd();e=Path(__file__).resolve().parent;out=e/'open-save';assert not out.exists();binary=r/'target/debug/ottrin-app'
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');dsk=importlib.util.module_from_spec(spec);spec.loader.exec_module(dsk)
report={'scope':'Actual Flatpak development sandbox/public FileChooser/frontend/source chooser/document export; not installed-browser or package acceptance','binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest()}
def wait(predicate):
 deadline=time.monotonic()+30
 while not predicate():assert time.monotonic()<deadline,'Sandbox QA readiness timeout';time.sleep(.05)
try:
 with dsk.OwnedDesktop(out) as d:
  fixture=d.profile/'fixture';fixture.mkdir();source=fixture/os.fsdecode(b'original\x80.txt');source.write_bytes(b'preserve sandbox exported document\n');original=source.read_bytes()
  app=d.profile/'sandbox';(app/'files/bin').mkdir(parents=True);(app/'metadata').write_text('[Application]\nname=org.ottrin.SandboxQA\nruntime=org.freedesktop.Platform/x86_64/25.08\nsdk=org.freedesktop.Platform/x86_64/25.08\n')
  flags=subprocess.check_output(['pkg-config','--cflags','--libs','gio-2.0'],text=True).split();subprocess.run(['cc','-Wall','-Wextra','-Werror',str(e/'client.c'),'-o',str(app/'files/bin/qa-client'),*flags],check=True)
  definitions=d.profile/'data/portal-definitions';definitions.mkdir();(definitions/'ottrin.portal').write_bytes((r/'packaging/linux/ottrin.portal').read_bytes())
  service=d.profile/'data/dbus-1/services/org.freedesktop.impl.portal.desktop.ottrin.service';service.parent.mkdir(parents=True);service.write_text('[D-BUS Service]\nName=org.freedesktop.impl.portal.desktop.ottrin\nExec="'+str(binary)+'" portal\n')
  config=d.profile/'config/xdg-desktop-portal/portals.conf';config.parent.mkdir();config.write_text('[preferred]\ndefault=none\norg.freedesktop.impl.portal.FileChooser=ottrin\n');(definitions/'portals.conf').write_bytes(config.read_bytes());d.env['XDG_DESKTOP_PORTAL_DIR']=str(definitions)
  runtime=d.profile/'portal-runtime';runtime.mkdir(mode=0o700);d.env['XDG_RUNTIME_DIR']=str(runtime)
  bus=d.spawn(['dbus-daemon','--session','--nofork','--print-address=1'],'portal-bus.log',pipe=True);d.env['DBUS_SESSION_BUS_ADDRESS']=d.ready_line(bus)
  keys=['DISPLAY','DBUS_SESSION_BUS_ADDRESS','XDG_RUNTIME_DIR','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_CACHE_HOME','XDG_STATE_HOME','HOME','XDG_DESKTOP_PORTAL_DIR','WINIT_UNIX_BACKEND','LIBGL_ALWAYS_SOFTWARE','NO_AT_BRIDGE'];dsk.run(['dbus-update-activation-environment',*[key for key in keys if key in d.env]],d.env)
  documents=d.spawn(['/usr/libexec/xdg-document-portal','--verbose'],'documents.log');mount=runtime/'doc'
  wait(lambda:mount.is_mount() or documents.poll() is not None);assert mount.is_mount() and documents.poll() is None,'Owned live private Documents mount required'
  frontend=d.spawn(['/usr/libexec/xdg-desktop-portal','--replace','--verbose'],'frontend.log')
  command=['flatpak','build','--runtime','--readonly','--die-with-parent','--nofilesystem=host','--nofilesystem=home','--unshare=network','--nosocket=session-bus','--nosocket=system-bus','--nosocket=x11','--nosocket=wayland','--talk-name=org.freedesktop.portal.Desktop','--bind-mount='+str(mount)+'='+str(mount),str(app),'/app/bin/qa-client',str(fixture)]
  report['sandbox_command']=command;client=d.spawn(command,'client.log');windows=[]
  def ready():
   assert client.poll() is None,(out/'client.log').read_text()
   windows[:]=dsk.run(['xdotool','search','--onlyvisible','--name','Sandbox portal Open QA'],d.env,False).stdout.split();return bool(windows)
  wait(ready);window=windows[-1];time.sleep(.3)
  # Real user navigation can access the host folder even though the caller cannot.
  dsk.run(['xdotool','windowactivate','--sync',window,'key','ctrl+l','type','--clearmodifiers',str(fixture)],d.env);dsk.run(['xdotool','key','Return'],d.env);time.sleep(.5)
  dsk.run(['import','-window',window,str(out/'chooser.png')],d.env)
  dsk.run(['xdotool','mousemove','--window',window,'300','152','click','1','mousemove','--window',window,'900','606','click','1'],d.env)
  client.wait(timeout=15);report['client_exit']=client.returncode;report['client_output']=(out/'client.log').read_text();assert client.returncode==0,report['client_output'];assert 'PASS exported_uri=' in report['client_output']
  save=d.spawn(command+['save'],'save-client.log');windows.clear()
  def save_ready():
   assert save.poll() is None,(out/'save-client.log').read_text()
   windows[:]=dsk.run(['xdotool','search','--onlyvisible','--name','Sandbox portal Save QA'],d.env,False).stdout.split();return bool(windows)
  wait(save_ready);window=windows[-1];time.sleep(.4);dsk.run(['import','-window',window,str(out/'save-chooser.png')],d.env)
  dsk.run(['xdotool','windowactivate','--sync',window,'mousemove','--window',window,'900','606','click','1'],d.env)
  save.wait(timeout=15);report['save_client_exit']=save.returncode;report['save_client_output']=(out/'save-client.log').read_text();assert save.returncode==0,report['save_client_output']
  assert (fixture/'saved-from-sandbox.txt').read_bytes()==b'saved by sandbox caller\n';report['caller_save_verified']=True
  assert source.read_bytes()==original;assert not (d.profile/'runtime/ottrin/index-v1.sock').exists();report.update(status='pass',original_preserved=True,no_index_started=True)
finally:
 report['teardown']=json.loads((out/'TEARDOWN.json').read_text()) if (out/'TEARDOWN.json').exists() else None;(out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
