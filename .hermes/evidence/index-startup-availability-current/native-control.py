"""Actual GUI availability with a declared non-ready index; owned desktop and finite lease."""
from pathlib import Path
import argparse,hashlib,importlib.util,json,os,subprocess,time
r=Path.cwd();e=Path(__file__).resolve().parent
parser=argparse.ArgumentParser();parser.add_argument('--expect',choices=['blocked','open'],required=True);args=parser.parse_args();out=e/args.expect;assert not out.exists()
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');desktop=importlib.util.module_from_spec(spec);spec.loader.exec_module(desktop)
source=r/'target/debug/ottrin-app';assert source.is_file() and not source.is_symlink()
def identity(p):
 s=p.stat();return {'device':s.st_dev,'inode':s.st_ino,'size':s.st_size,'mtime_ns':s.st_mtime_ns,'links':s.st_nlink}
before=identity(source)
with source.open('rb') as f:digest=hashlib.file_digest(f,'sha256').hexdigest()
report={'expectation':args.expect,'binary_sha256':digest,'source_binary_before':before,'scope':'Actual debug-source GUI under a non-ready package-owned test index; not package or resource acceptance'}
try:
 with desktop.OwnedDesktop(out) as d:
  fixture=d.profile/'fixture';fixture.mkdir();(fixture/'original.txt').write_text('preserve original\n')
  runtime=d.profile/'runtime-bin';runtime.mkdir();executable=runtime/'ottrin-app';os.link(source,executable)
  fake=runtime/'ottrin-indexd';fake.write_text('#!/usr/bin/python3\nfrom pathlib import Path\nimport os,time\nPath(__file__).with_name("INDEX_STARTED").write_text(str(os.getpid()))\ntime.sleep(60)\n');fake.chmod(0o700)
  config=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];config.update(restore_session=False,last_session_dirs=[],installed_office_previews_enabled=False,theme='Dark');config['bookmarks']=[];config['search'].update(include_roots=[str(fixture)],content_indexing_enabled=False)
  path=d.profile/'config/ottrin/config.json';path.parent.mkdir(parents=True);path.write_text(json.dumps(config)+'\n')
  started=time.monotonic();app=d.spawn([str(executable),str(fixture)],'app.log');windows=[]
  deadline=started+(2 if args.expect=='blocked' else 5)
  while time.monotonic()<deadline and app.poll() is None:
   q=desktop.run(['xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','Ottrin'],d.env,False);windows=q.stdout.strip().splitlines()
   if windows:break
   time.sleep(.05)
  report['visible_within_deadline']=bool(windows);report['observed_ms']=round((time.monotonic()-started)*1000,2)
  if args.expect=='blocked':
   assert not windows,'Negative control no longer reproduces; inspect changed source'
   app.wait(timeout=35)
   report['exit']=app.returncode;report['stderr']=(out/'app.log').read_text()
   assert app.returncode==1 and 'index daemon did not become ready within 30 seconds' in report['stderr']
   report['status']='reproduced: GUI never opens and exits for a non-ready index'
  else:
   assert windows,'File manager must open while indexing is unavailable'
   window=windows[-1];desktop.run(['xdotool','windowactivate','--sync',window],d.env);time.sleep(.5)
   desktop.run(['import','-window',window,str(out/'open.png')],d.env)
   desktop.run(['xdotool','key','--clearmodifiers','alt+F4'],d.env);app.wait(timeout=8);assert app.returncode==0
   report['exit']=app.returncode;report['status']='pass: normal GUI open and close with non-ready index'
  report['test_index_started']=(runtime/'INDEX_STARTED').exists();assert report['test_index_started']
  assert not (d.profile/'runtime/ottrin/index-v1.sock').exists();report['index_never_became_ready']=True
  assert (fixture/'original.txt').read_text()=='preserve original\n';report['original_bytes_preserved']=True
finally:
 report['teardown']=json.loads((out/'TEARDOWN.json').read_text()) if (out/'TEARDOWN.json').exists() else None
 report['source_binary_after']=identity(source);assert report['source_binary_after']==before
 (out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
