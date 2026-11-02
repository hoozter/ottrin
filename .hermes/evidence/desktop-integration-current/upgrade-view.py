"""Previous -> staged AppImage -> reopen, preserving Miller and private files."""
from pathlib import Path
import hashlib, importlib.util, json, time
r=Path.cwd(); e=Path(__file__).resolve().parent; out=e/'upgrade-view'; assert not out.exists()
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py'); m=importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
new=Path(json.loads((e/'BUILD_SOURCE.json').read_text())['stage'])/'Ottrin-0.1.0-x86_64.AppImage'
old=r/'dist/packages'/new.name
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
report={'old_sha256':sha(old),'artifact_sha256':sha(new),'sessions':[]}
assert report['artifact_sha256']==json.loads((e/'PACKAGE_CONTENT_ACCEPTANCE.json').read_text())['artifacts'][new.name]
try:
 with m.OwnedDesktop(out) as d:
  fixture=d.profile/'fixture'; fixture.mkdir(); (fixture/'folder').mkdir(); source=fixture/'preserve.txt'; source.write_text('Untouched upgrade fixture\n')
  cfg=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];cfg.update(default_view_mode='Miller',theme='Dark',bookmarks=[],installed_office_previews_enabled=False);cfg['search'].update(include_roots=[str(fixture)],content_indexing_enabled=False)
  cp=d.profile/'config/ottrin/config.json';cp.parent.mkdir(parents=True);cp.write_text(json.dumps(cfg)+'\n')
  for label,image in [('previous',old),('updated',new),('reopened',new)]:
   app=d.spawn([str(image),str(fixture),'--renderer','opengl'],label+'.log');deadline=time.monotonic()+25
   while True:
    assert app.poll() is None
    found=m.run(['xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','^Ottrin$'],d.env,False).stdout.split()
    if found:break
    assert time.monotonic()<deadline;time.sleep(.05)
   w=found[-1];m.run(['xdotool','windowactivate','--sync',w],d.env);time.sleep(.8)
   m.run(['import','-window',w,str(out/(label+'.png'))],d.env)
   geom=dict(line.split('=',1) for line in m.run(['xdotool','getwindowgeometry','--shell',w],d.env).stdout.splitlines() if '=' in line)
   m.run(['xdotool','mousemove','--window',w,str(int(geom['WIDTH'])-22),'18'],d.env);time.sleep(.1);m.run(['xdotool','mousedown','1'],d.env)
   try:time.sleep(.06)
   finally:m.run(['xdotool','mouseup','1'],d.env)
   app.wait(timeout=10);assert app.returncode==0
   saved=json.loads(cp.read_text());assert saved['default_view_mode']=='Miller';assert source.read_text()=='Untouched upgrade fixture\n'
   report['sessions'].append({'label':label,'normal_exit':0,'saved_view':'Miller','image_sha256':sha(image)})
  report.update(status='pass',scope='Actual previous/update/reopen AppImage in one private profile; screenshots require visual confirmation of Miller columns')
finally:
 report['teardown']=json.loads((out/'TEARDOWN.json').read_text()) if (out/'TEARDOWN.json').exists() else None
 (out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
