"""Actual Wayland activation protocol on an owned virtual compositor, not a physical seat."""
from pathlib import Path
import importlib.util,os,time,json,hashlib
r=Path.cwd();e=Path(__file__).resolve().parent;out=e/'wayland-egl';assert not out.exists();b=r/'target/debug/ottrin-app'
s=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');m=importlib.util.module_from_spec(s);s.loader.exec_module(m)
report={'scope':'Virtual KWin Wayland source Properties, real surface/activation request; synthetic token does not establish compositor grant for a real caller','binary_sha256':hashlib.sha256(b.read_bytes()).hexdigest()}
def wait(test):
 end=time.monotonic()+25
 while not test():assert time.monotonic()<end,'Owned Wayland readiness timeout';time.sleep(.05)
try:
 with m.OwnedDesktop(out) as d:
  # Keep the outer X11 manager on its original bus. Nested compositor gets a private bus.
  bus=d.spawn(['dbus-daemon','--session','--nofork','--print-address=1'],'nested-dbus.log',pipe=True);d.env['DBUS_SESSION_BUS_ADDRESS']=d.ready_line(bus);d.env['KWIN_COMPOSE']='O2';d.env.pop('QT_QPA_PLATFORM',None);d.env.pop('WAYLAND_DISPLAY',None);d.env.update(QT_LOGGING_TO_CONSOLE='1',QT_FORCE_STDERR_LOGGING='1',QT_LOGGING_RULES='kwin_scripting.debug=true')
  compositor=d.spawn(['kwin_wayland','--virtual','--socket','ottrin-wayland-qa','--width','1024','--height','768','--no-lockscreen','--no-global-shortcuts','--no-kactivities'],'compositor.log')
  def socket_ready():assert compositor.poll() is None,(out/'compositor.log').read_text();return (d.profile/'runtime/ottrin-wayland-qa').exists()
  wait(socket_ready)
  d.env.update(WAYLAND_DISPLAY='ottrin-wayland-qa',WINIT_UNIX_BACKEND='wayland',QT_QPA_PLATFORM='wayland',WAYLAND_DEBUG='client',XDG_ACTIVATION_TOKEN='ottrin-owned-wayland-protocol-token')
  source=d.profile/'fixture.txt';source.write_bytes(b'Wayland activation must preserve this file.\n');original=source.read_bytes()
  app=d.spawn([str(b),'--properties','--',str(source)],'app.log')
  def activated():assert app.poll() is None,(out/'app.log').read_text()[-5000:];return 'xdg_activation_v1' in (out/'app.log').read_text() and '.activate(' in (out/'app.log').read_text()
  wait(activated);time.sleep(1)
  script=d.profile/'inspect-window.js';script.write_text("for (const w of workspace.windowList()) { if (w.pid === "+str(app.pid)+") { print('OTTRIN-WAYLAND-WINDOW:'+JSON.stringify({pid:w.pid,caption:w.caption,width:w.frameGeometry.width,height:w.frameGeometry.height,active:workspace.activeWindow===w})); w.closeWindow(); }}")
  inspect=m.run(['gdbus','introspect','--session','--dest','org.kde.KWin','--object-path','/Scripting'],d.env);(out/'scripting-interface.txt').write_text(inspect.stdout)
  loaded=m.run(['gdbus','call','--session','--dest','org.kde.KWin','--object-path','/Scripting','--method','org.kde.kwin.Scripting.loadScript',str(script)],d.env)
  import re
  script_id=int(re.search(r'(?:int32 )?(-?\d+)',loaded.stdout).group(1));assert script_id>=0,loaded.stdout
  inspected=m.run(['gdbus','introspect','--session','--dest','org.kde.KWin','--object-path',f'/Scripting/Script{script_id}'],d.env);(out/'script-interface.txt').write_text(inspected.stdout)
  m.run(['gdbus','call','--session','--dest','org.kde.KWin','--object-path',f'/Scripting/Script{script_id}','--method','org.kde.kwin.Script.run'],d.env)
  app.wait(timeout=15);assert app.returncode==0,(out/'app.log').read_text()[-3000:]
  compositor_log=(out/'compositor.log').read_text();marker='OTTRIN-WAYLAND-WINDOW:';lines=[line.split(marker,1)[1] for line in compositor_log.splitlines() if marker in line];assert lines,compositor_log[-4000:]
  window=json.loads(lines[-1]);assert window['pid']==app.pid and window['width']>=480 and window['height']>=360 and 'Properties' in window['caption'];report['native_window']=window
  log=(out/'app.log').read_text();assert 'ottrin-owned-wayland-protocol-token' in log;assert "thread 'main'" not in log;assert 'desktop startup notification:' not in log
  assert source.read_bytes()==original;assert not list((d.profile/'runtime').rglob('index*.sock'));assert not (d.profile/'config/ottrin/config.json').exists()
  report.update(status='pass',native_wayland_activation_sent=True,normal_close=True,source_preserved=True,no_index_or_preferences=True,compositor_pid=compositor.pid,app_pid=app.pid)
finally:
 report['teardown']=json.loads((out/'TEARDOWN.json').read_text()) if (out/'TEARDOWN.json').exists() else None;(e/'WAYLAND_RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
