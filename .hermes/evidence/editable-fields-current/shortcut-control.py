from pathlib import Path
import importlib.util,json,time
r=Path.cwd();e=Path(__file__).resolve().parent;out=e/'shortcut-control'
s=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');m=importlib.util.module_from_spec(s);s.loader.exec_module(m)
report={'scope':'Independent stock xmessage under the same private X11/KWin fixture; no Ottrin process or product shortcut acceptance'}
try:
 with m.OwnedDesktop(out) as d:
  p=d.spawn(['xmessage','-title','Owned shortcut control','-buttons','Close:0','Owned shortcut control'],'control.log');deadline=time.monotonic()+15
  while True:
   windows=m.run(['xdotool','search','--onlyvisible','--pid',str(p.pid),'--name','Owned shortcut control'],d.env,False).stdout.split()
   if windows:break
   assert time.monotonic()<deadline;time.sleep(.1)
  w=windows[-1];m.run(['xdotool','windowactivate','--sync',w],d.env)
  active=m.run(['xdotool','getactivewindow'],d.env).stdout.strip();assert active==w,(active,w)
  bus=m.run(['gdbus','call','--session','--dest','org.freedesktop.DBus','--object-path','/org/freedesktop/DBus','--method','org.freedesktop.DBus.ListNames'],d.env);report['bus_names']=bus.stdout
  shortcuts=m.run(['qdbus6','org.kde.kglobalaccel','/component/kwin','org.kde.kglobalaccel.Component.shortcutNames'],d.env,False);report['kwin_shortcuts']={'exit':shortcuts.returncode,'stdout':shortcuts.stdout,'stderr':shortcuts.stderr}
  m.run(['xdotool','key','--clearmodifiers','alt+F4'],d.env);deadline=time.monotonic()+4
  while p.poll() is None and time.monotonic()<deadline:time.sleep(.1)
  report['alt_f4_exit']=p.poll();report['status']='stock app closed via private Alt+F4' if p.poll() is not None else 'stock app also remains open after private Alt+F4; prior Ottrin-only attribution disproven for this fixture'
finally:
 (out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
