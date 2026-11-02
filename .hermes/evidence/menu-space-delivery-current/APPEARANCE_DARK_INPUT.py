"""Owned native appearance audit; every input targets an owned PID/window."""
from pathlib import Path
import argparse,hashlib,importlib.util,json,signal,time,sys,re
r=Path.cwd();parser=argparse.ArgumentParser();parser.add_argument('--case',choices=['dark100','light150'],required=True);parser.add_argument('--output',type=Path,required=True);args=parser.parse_args();e=args.output.resolve();out=e/('appearance-'+args.case);assert not out.exists();scale=1.5 if args.case=='light150' else 1
signal.signal(signal.SIGTERM,lambda *_:(_ for _ in ()).throw(KeyboardInterrupt()))
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');desktop=importlib.util.module_from_spec(spec);spec.loader.exec_module(desktop)
b=json.loads((e/'BUILD_RESULT.json').read_text());image=Path(b['stage'])/'Ottrin-0.1.0-x86_64.AppImage';sha=lambda p:hashlib.file_digest(p.open('rb'),'sha256').hexdigest();assert sha(image)==b['artifacts'][image.name]
report={'status':'incomplete','source_revision':b['source_revision'],'artifact_sha256':sha(image),'scope':'Actual packaged Settings/theme editor native pointer/keyboard audit on owned X11/software desktop','case':args.case,'actions':[]}
try:
 with desktop.OwnedDesktop(out,scale=str(scale)) as d:
  fixture=d.profile/'fixture';fixture.mkdir();source=fixture/'preserve.txt';source.write_bytes(b'Appearance audit fixture\n')
  config=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];config.update(theme='Light' if scale==1.5 else 'Dark',default_view_mode='List',bookmarks=[],installed_office_previews_enabled=False);config['search'].update(include_roots=[str(fixture)],content_indexing_enabled=False)
  cp=d.profile/'config/ottrin/config.json';cp.parent.mkdir(parents=True);cp.write_text(json.dumps(config)+'\n')
  app=d.spawn([str(image),str(fixture),'--renderer','opengl'],'app.log')
  def windows():
   ids=desktop.run(['xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','Ottrin'],d.env,False).stdout.split();return {desktop.run(['xdotool','getwindowname',w],d.env).stdout.strip():w for w in ids}
  deadline=time.monotonic()+25
  while 'Ottrin' not in windows():assert app.poll() is None and time.monotonic()<deadline;time.sleep(.1)
  time.sleep(.8)
  def state():print(json.dumps({'windows':windows(),'profile':str(d.profile),'scale':scale}),flush=True)
  state()
  for line in sys.stdin:
   a=json.loads(line);op=a['op'];report['actions'].append(a)
   if op=='finish':
    w=windows()['Ottrin'];g=dict(x.split('=',1) for x in desktop.run(['xdotool','getwindowgeometry','--shell',w],d.env).stdout.splitlines() if '=' in x);desktop.run(['xdotool','windowactivate','--sync',w,'mousemove','--window',w,str(int(g['WIDTH'])-round(22*scale)),str(round(18*scale)),'click','1'],d.env);app.wait(timeout=15);assert app.returncode==0
    assert source.read_bytes()==b'Appearance audit fixture\n';after=json.loads(cp.read_text());assert after==config,(after,config)
    report.update(status='native actions complete; visual review required',normal_exit=0,source_and_config_preserved=True);break
   w=windows()[a.get('window','Ottrin')];desktop.run(['xdotool','windowactivate','--sync',w],d.env);time.sleep(.15)
   if op=='click':
    desktop.run(['xdotool','mousemove','--window',w,str(round(a['x']*scale)),str(round(a['y']*scale))],d.env);time.sleep(.15);desktop.run(['xdotool','mousedown','1'],d.env);time.sleep(.08);desktop.run(['xdotool','mouseup','1'],d.env)
   elif op=='key':desktop.run(['xdotool','key','--clearmodifiers',a['value']],d.env)
   elif op=='text':desktop.run(['xdotool','type','--clearmodifiers','--delay','50',a['value']],d.env)
   elif op=='capture':
    assert re.fullmatch('[a-z0-9-]+',a['name']);desktop.run(['import','-window',w,str(out/(a['name']+'.png'))],d.env)
   else:raise ValueError(op)
   time.sleep(.35);state()
finally:
 report['teardown']=json.loads((out/'TEARDOWN.json').read_text()) if (out/'TEARDOWN.json').exists() else None;(out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
