"""Observe popup keyboard isolation through owned native input; synthetic files only."""
from pathlib import Path
import argparse,hashlib,importlib.util,json,time
from PIL import Image
r=Path.cwd();parser=argparse.ArgumentParser();parser.add_argument('--cohort',type=Path,required=True);parser.add_argument('--output',type=Path,required=True);args=parser.parse_args();e=args.output.resolve();assert e.is_relative_to(r/'.hermes/evidence');e.mkdir(exist_ok=True);out=e/'desktop';assert not out.exists()
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');desktop=importlib.util.module_from_spec(spec);spec.loader.exec_module(desktop)
cohort=args.cohort.resolve();assert cohort.is_relative_to(r/'.hermes/evidence');build=json.loads((cohort/'BUILD_RESULT.json').read_text());image=Path(build['stage'])/'Ottrin-0.1.0-x86_64.AppImage';digest=hashlib.file_digest(image.open('rb'),'sha256').hexdigest();assert digest==build['artifacts'][image.name]
report={'status':'observing','artifact_sha256':digest,'source_revision':build['source_revision'],'scope':'Private native X11 synthetic-file main/Inspector menu keyboard observation; no product mutation or personal profile'}
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
  click(main,width(main)-26,54);capture(main,'main-menu-open');key(main,'Down');capture(main,'main-menu-after-down');key(main,'Escape');capture(main,'main-after-popup-escape')
  samples={name:[Image.open(out/(name+'.png')).convert('RGB').getpixel((300,y)) for y in [114,146]] for name in ['main-selected-before','main-menu-open','main-menu-after-down','main-after-popup-escape']}
  selected,plain=samples['main-menu-open'];assert selected!=plain
  distance=lambda a,b:sum((x-y)**2 for x,y in zip(a,b))
  for name in ['main-menu-after-down','main-after-popup-escape']:
   first,second=samples[name];assert distance(first,selected)<distance(first,plain),(name,samples);assert distance(second,plain)<distance(second,selected),(name,samples)
  report['main_selection_palette_samples']=samples;report['main_selection_retained_behind_menu']=True
  click(main,120,114);key(main,'space');inspector=wait_window('^Ottrin Quick Inspector');capture(inspector,'inspector-before')
  click(inspector,width(inspector)-32,70);capture(inspector,'inspector-menu-open');key(inspector,'Escape')
  remaining=windows('^Ottrin Quick Inspector');report['inspector_remained_open_after_popup_escape']=inspector in remaining
  assert inspector in remaining,'Popup Escape closed the Inspector'
  capture(inspector,'inspector-after-popup-escape');key(inspector,'Escape');assert not windows('^Ottrin Quick Inspector'),'Second Escape must retain normal Inspector close';report['second_escape_closes_inspector']=True
  capture(main,'main-final');click(main,width(main)-22,18);app.wait(timeout=10);assert app.returncode==0
  assert all((fixture/name).read_bytes()==payload for name in ['alpha.txt','beta.txt'])
  report.update(status='pass: native menus own arrows/Escape; closed-menu Inspector Escape still closes normally',normal_exit=0,source_files_unchanged=True)
finally:
 report['teardown']=json.loads((out/'TEARDOWN.json').read_text()) if (out/'TEARDOWN.json').exists() else None;(e/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
