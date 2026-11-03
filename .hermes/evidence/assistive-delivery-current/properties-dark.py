"""Exact AppImage Properties at Dark100; own desktop and preserve source/config."""
from pathlib import Path
import hashlib,importlib.util,json,time
r=Path.cwd();e=Path(__file__).resolve().parent;out=e/'properties-dark100';assert not out.exists()
s=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');m=importlib.util.module_from_spec(s);s.loader.exec_module(m)
b=Path(json.loads((e/'BUILD_SOURCE.json').read_text())['stage'])/'Ottrin-0.1.0-x86_64.AppImage';digest=hashlib.sha256(b.read_bytes()).hexdigest();assert digest==json.loads((e/'PACKAGE_CONTENT_ACCEPTANCE.json').read_text())['artifacts'][b.name]
report={'artifact_sha256':digest,'scope':'Actual packaged Dark100 Properties, long metadata, normal native-pointer Close, no index/config/source mutation'}
try:
 with m.OwnedDesktop(out,scale='1') as d:
  source=d.profile/('long metadata name '*8+'report.txt');source.write_bytes(b'Properties is read only.\n')
  config=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];config.pop('restore_session',None);config.pop('last_session_dirs',None);config['theme']='Dark'
  cp=d.profile/'config/ottrin/config.json';cp.parent.mkdir();cp.write_text(json.dumps(config)+'\n');before=cp.read_bytes()
  app=d.spawn([str(b),'--properties',str(source)],'app.log');end=time.monotonic()+20
  while True:
   assert app.poll() is None
   found=m.run(['xdotool','search','--onlyvisible','--pid',str(app.pid),'--name','^Ottrin'],d.env,False).stdout.split()
   if found:break
   assert time.monotonic()<end;time.sleep(.05)
  w=found[-1];m.run(['xdotool','windowactivate','--sync',w],d.env);time.sleep(.6);m.run(['import','-window',w,str(out/'initial.png')],d.env)
  m.run(['xdotool','mousemove','--window',w,'500','400','click','--repeat','6','--delay','80','5'],d.env);time.sleep(.3);m.run(['import','-window',w,str(out/'bottom.png')],d.env)
  geometry=dict(line.split('=',1) for line in m.run(['xdotool','getwindowgeometry','--shell',w],d.env).stdout.splitlines() if '=' in line)
  m.run(['xdotool','mousemove','--window',w,str(int(geometry['WIDTH'])-22),'18'],d.env);time.sleep(.1)
  m.run(['xdotool','mousedown','1'],d.env);time.sleep(.06);m.run(['xdotool','mouseup','1'],d.env);time.sleep(.1)
  app.wait(timeout=5);assert app.returncode==0
  report['native_close_button_clicked']=True
  assert source.read_bytes()==b'Properties is read only.\n' and cp.read_bytes()==before
  assert not list((d.profile/'runtime').rglob('index*.sock'))
  report.update(status='pass',normal_exit=0,source_and_config_preserved=True,no_index_started=True)
finally:
 report['teardown']=json.loads((out/'TEARDOWN.json').read_text()) if (out/'TEARDOWN.json').exists() else None;(out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
