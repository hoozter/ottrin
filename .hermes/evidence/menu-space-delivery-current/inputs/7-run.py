"""Observe popup keyboard isolation through owned native input; synthetic files only."""
from pathlib import Path
import argparse,hashlib,importlib.util,json,time
r=Path.cwd();parser=argparse.ArgumentParser();parser.add_argument('--cohort',type=Path,required=True);parser.add_argument('--output',type=Path,required=True);args=parser.parse_args();e=args.output.resolve();e.mkdir(parents=True,exist_ok=True);out=e/'desktop';assert not out.exists()
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');desktop=importlib.util.module_from_spec(spec);spec.loader.exec_module(desktop)
cohort=args.cohort.resolve();build=json.loads((cohort/'BUILD_RESULT.json').read_text());image=Path(build['stage'])/'Ottrin-0.1.0-x86_64.AppImage';digest=hashlib.file_digest(image.open('rb'),'sha256').hexdigest();assert digest==build['artifacts'][image.name]
report={'status':'observing','artifact_sha256':digest,'source_revision':build['source_revision'],'scope':'Private native X11 synthetic-file main/Inspector popup Space ownership observation; no product mutation or personal profile'}
try:
 with desktop.OwnedDesktop(out) as d:
  fixture=d.profile/'fixture';fixture.mkdir();payload=b'Read-only keyboard popup fixture.\n'
  for name in ['alpha.txt','beta.txt']:(fixture/name).write_bytes(payload)
  config=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];config.update(theme='Dark',default_view_mode='List',bookmarks=[],installed_office_previews_enabled=False);config['search'].update(include_roots=[str(fixture)],content_indexing_enabled=False)
  cp=d.profile/'config/ottrin/config.json';cp.parent.mkdir(parents=True);cp.write_text(json.dumps(config)+'\n')
  app=d.spawn([str(image),str(fixture),'--renderer','opengl'],'app.log')
  def windows(pattern):return desktop.run(['xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name',pattern],d.env,False).stdout.split()
  def wait_window(pattern):
   deadline=time.monotonic()+20
   while True:
    assert app.poll() is None
    ids=windows(pattern)
    if ids:return ids[-1]
    assert time.monotonic()<deadline,pattern;time.sleep(.1)
  def capture(w,name):desktop.run(['import','-window',w,str(out/(name+'.png'))],d.env)
  def click(w,x,y):
   desktop.run(['xdotool','windowactivate','--sync',w,'mousemove','--window',w,str(x),str(y)],d.env);time.sleep(.15);desktop.run(['xdotool','mousedown','1'],d.env);time.sleep(.1);desktop.run(['xdotool','mouseup','1'],d.env);time.sleep(.3)
  def key(w,value):desktop.run(['xdotool','windowactivate','--sync',w],d.env);time.sleep(.15);desktop.run(['xdotool','key','--clearmodifiers',value],d.env);time.sleep(.3)
  def width(w):return int(dict(line.split('=',1) for line in desktop.run(['xdotool','getwindowgeometry','--shell',w],d.env).stdout.splitlines() if '=' in line)['WIDTH'])
  main=wait_window('^Ottrin$');time.sleep(.7);click(main,120,114);capture(main,'main-selected-before')
  click(main,width(main)-26,54);capture(main,'main-menu-before-space');key(main,'space')
  time.sleep(2);ids=windows('^Ottrin Quick Inspector');report['main_menu_space_opened_inspector']=bool(ids);capture(main,'main-after-menu-space')
  assert not ids,'Space opened Inspector behind main menu'
  key(main,'Escape');key(main,'Escape')
  click(main,120,114);click(main,120,114);key(main,'space');inspector=wait_window('^Ottrin Quick Inspector');capture(inspector,'inspector-before')
  click(inspector,width(inspector)-32,70);capture(inspector,'inspector-menu-before-space');key(inspector,'space')
  remaining=windows('^Ottrin Quick Inspector');report['inspector_menu_space_closed_inspector']=inspector not in remaining
  assert inspector in remaining,'Space closed Inspector behind its menu'
  capture(inspector,'inspector-after-menu-space');key(inspector,'Escape');key(inspector,'space');time.sleep(.5)
  assert not windows('^Ottrin Quick Inspector'),'Normal Space did not close Inspector'
  report['normal_space_opens_and_closes_inspector']=True
  capture(main,'main-final');click(main,width(main)-22,18);app.wait(timeout=10);assert app.returncode==0
  assert sorted(x.name for x in fixture.iterdir())==['alpha.txt','beta.txt']
  assert all((fixture/name).read_bytes()==payload for name in ['alpha.txt','beta.txt'])
  report.update(status='pass',normal_exit=0,source_files_unchanged=True)
finally:
 report['teardown']=json.loads((out/'TEARDOWN.json').read_text()) if (out/'TEARDOWN.json').exists() else None;(e/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
