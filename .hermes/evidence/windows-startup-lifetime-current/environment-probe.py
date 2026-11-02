"""Check inherited Wayland settings cannot redirect a private X11 child."""
from pathlib import Path
import importlib.util,json,os
r=Path.cwd();e=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');desktop=importlib.util.module_from_spec(spec);spec.loader.exec_module(desktop)
with desktop.OwnedDesktop(e/'environment-probe') as d:
 assert all(d.env[k]=='x11' for k in ['EGL_PLATFORM','GDK_BACKEND','XDG_SESSION_TYPE'])
 assert 'WAYLAND_DISPLAY' not in d.env and 'WAYLAND_SOCKET' not in d.env
 p=d.spawn(['python3','-c','import json,os;print(json.dumps({k:os.environ.get(k) for k in ["DISPLAY","EGL_PLATFORM","GDK_BACKEND","XDG_SESSION_TYPE","WAYLAND_DISPLAY","WAYLAND_SOCKET"]}))'],'child.json');assert p.wait(timeout=5)==0
 actual=json.loads((e/'environment-probe/child.json').read_text());assert actual['DISPLAY']==d.env['DISPLAY'];assert actual['EGL_PLATFORM']==actual['GDK_BACKEND']==actual['XDG_SESSION_TYPE']=='x11';assert actual['WAYLAND_DISPLAY'] is None and actual['WAYLAND_SOCKET'] is None
 desktop.run(['xdpyinfo'],d.env)
 (e/'environment-probe/RESULT.json').write_text(json.dumps({'status':'pass','scope':'Actual private X11 display plus child environment after inherited Wayland values; no host display or GUI viewer input','actual':actual},indent=2)+'\n')
