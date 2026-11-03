from pathlib import Path
import importlib.util,json,time,subprocess,hashlib,sys
r=Path.cwd();e=r/'.hermes/evidence/release-audit-current'/(sys.argv[2] if len(sys.argv)>2 else 'startup-baseline');e.mkdir(exist_ok=True)
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
b=Path(sys.argv[1]).resolve() if len(sys.argv)>1 else r/'dist/packages/Ottrin-0.1.0-x86_64.AppImage';report={'artifact_sha256':hashlib.file_digest(b.open('rb'),'sha256').hexdigest(),'scope':'No-argument startup from deeply nested disposable HOME; native screenshot only, not a workflow pass'}
try:
 with m.OwnedDesktop(e,scale=sys.argv[4] if len(sys.argv)>4 else '1') as d:
  home=d.profile/'home';(home/'Documents').mkdir(parents=True);(home/'Documents/proof.txt').write_text('Private startup proof.\n')
  config=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];config.update(default_view_mode='Miller',theme=sys.argv[3] if len(sys.argv)>3 else 'Dark',bookmarks=[]);config['search'].update(include_roots=[str(home)],content_indexing_enabled=False);p=d.profile/'config/ottrin/config.json';p.parent.mkdir(parents=True);p.write_text(json.dumps(config))
  app=d.spawn([str(b),'--renderer','opengl'],'app.log');deadline=time.monotonic()+25;w=None
  while time.monotonic()<deadline:
   q=subprocess.run(['xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','^Ottrin$'],env=d.env,capture_output=True,text=True,timeout=10)
   if q.returncode==0:w=q.stdout.split()[-1];break
   assert app.poll() is None;time.sleep(.1)
  assert w;time.sleep(1);subprocess.run(['import','-window',w,str(e/'startup.png')],env=d.env,check=True,timeout=15);report['native_window']=w;report['home']=str(home)
finally:
 report['teardown']=json.loads((e/'TEARDOWN.json').read_text());(e/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
