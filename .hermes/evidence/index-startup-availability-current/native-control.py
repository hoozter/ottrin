"""Actual GUI availability with a declared non-ready index; owned desktop and finite lease."""
from pathlib import Path
import argparse,hashlib,importlib.util,json,os,subprocess,time
r=Path.cwd();e=Path(__file__).resolve().parent
parser=argparse.ArgumentParser();parser.add_argument('--expect',choices=['blocked','open'],required=True);parser.add_argument('--case',default='');parser.add_argument('--preview',action='store_true');parser.add_argument('--complete-after-close',action='store_true');args=parser.parse_args();assert not(args.preview and args.complete_after_close);out=e/(('complete-' if args.complete_after_close else 'preview-' if args.preview else '')+args.expect+args.case);assert not out.exists()
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
  if args.expect=='open' and not args.preview and not args.complete_after_close:
   (fixture/'subfolder').mkdir();(fixture/'subfolder/inside.txt').write_text('preserve child\n')
  runtime=d.profile/'runtime-bin';runtime.mkdir();executable=runtime/'ottrin-app';os.link(source,executable)
  fake=runtime/'ottrin-indexd'
  script='#!/usr/bin/python3\nfrom pathlib import Path\nimport os,time,json\nPath(__file__).with_name("INDEX_STARTED").write_text(json.dumps({"index_pid":os.getpid(),"broker_pid":os.getppid()}))\ntime.sleep('+('5' if args.complete_after_close else '60')+')\n'
  if args.complete_after_close:script+='os.execv('+repr(str(r/'target/debug/ottrin-indexd'))+',["ottrin-indexd","--index-daemon"])\n'
  fake.write_text(script);fake.chmod(0o700)
  config=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];config.update(restore_session=False,last_session_dirs=[],installed_office_previews_enabled=False,theme='Dark');config['bookmarks']=[];config['search'].update(include_roots=[str(fixture)],content_indexing_enabled=False)
  path=d.profile/'config/ottrin/config.json';path.parent.mkdir(parents=True);path.write_text(json.dumps(config)+'\n')
  if args.expect=='open':
   config['default_view_mode']='List';path.write_text(json.dumps(config)+'\n')
  started=time.monotonic();app=d.spawn([str(executable),*(['preview',str(fixture/'original.txt')] if args.preview else [str(fixture)])],'app.log');windows=[]
  deadline=started+(2 if args.expect=='blocked' else 5)
  while time.monotonic()<deadline and app.poll() is None:
   q=desktop.run(['xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','.*'],d.env,False);windows=q.stdout.strip().splitlines()
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
   if not args.preview and not args.complete_after_close:
    while time.monotonic()-started<33:
     assert app.poll() is None,'Index readiness failure must not close the file manager';time.sleep(.2)
    report['survived_index_readiness_timeout']=True
    desktop.run(['import','-window',window,str(out/'after-index-timeout.png')],d.env)
    desktop.run(['xdotool','windowactivate','--sync',window],d.env)
    assert desktop.run(['xdotool','getactivewindow'],d.env).stdout.strip()==window
    desktop.run(['xdotool','key','--clearmodifiers','ctrl+l'],d.env);time.sleep(.2)
    desktop.run(['xdotool','type','--clearmodifiers','--delay','2',str(fixture/'subfolder')],d.env)
    desktop.run(['xdotool','key','--clearmodifiers','Return'],d.env);time.sleep(.8)
    desktop.run(['import','-window',window,str(out/'responsive-browsing.png')],d.env)
    assert (fixture/'subfolder/inside.txt').read_text()=='preserve child\n'

   geometry=desktop.run(['xdotool','getwindowgeometry','--shell',window],d.env).stdout
   width=int(dict(line.split('=',1) for line in geometry.splitlines())['WIDTH'])
   desktop.run(['xdotool','mousemove','--sync','--window',window,str(width-24),'18','click','1'],d.env)
   try:app.wait(timeout=8)
   except subprocess.TimeoutExpired:
    report['windows_after_close']=desktop.run(['xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','.*'],d.env,False).stdout
    report['threads_after_close']={t.name:(t/'wchan').read_text() for t in (Path('/proc')/str(app.pid)/'task').iterdir()}
    desktop.run(['import','-window','root',str(out/'close-timeout.png')],d.env)
    trace=subprocess.run(['gdb','--batch','-p',str(app.pid),'-ex','thread apply all bt','-ex','detach'],capture_output=True,text=True,timeout=20)
    (out/'close-backtrace.log').write_text(trace.stdout+trace.stderr)
    raise
   assert app.returncode==0
   report['exit']=app.returncode;report['status']='pass: normal GUI open and close with non-ready index'
   if args.complete_after_close:
    marker=runtime/'INDEX_STARTED';deadline=time.monotonic()+2
    while not marker.exists():assert time.monotonic()<deadline;time.sleep(.05)
    ids=json.loads(marker.read_text());broker=Path('/proc')/str(ids['broker_pid'])
    assert broker.exists() and (broker/'stat').read_text().rsplit(') ',1)[1].split()[0]!='Z'
    assert os.fsencode('XDG_RUNTIME_DIR='+str(d.profile/'runtime')) in (broker/'environ').read_bytes().split(b'\0')
    report['independent_bootstrap_outlived_window']=True
    deadline=time.monotonic()+15
    while not (d.profile/'runtime/ottrin/index-v1.sock').exists():assert time.monotonic()<deadline;time.sleep(.05)
    deadline=time.monotonic()+15
    while True:
     status=desktop.run([str(executable),'index','status','--json'],d.env);report['index_status_after_window_close']=json.loads(status.stdout)
     if report['index_status_after_window_close']['status'].casefold()=='ready':break
     assert time.monotonic()<deadline;time.sleep(.1)

    deadline=time.monotonic()+5
    while broker.exists() and (broker/'stat').read_text().rsplit(') ',1)[1].split()[0]!='Z':assert time.monotonic()<deadline;time.sleep(.05)
    daemon=Path('/proc')/str(ids['index_pid']);assert daemon.exists();assert os.fsencode('XDG_RUNTIME_DIR='+str(d.profile/'runtime')) in (daemon/'environ').read_bytes().split(b'\0')
    report['bootstrap_finished_and_index_remained_running']=True

  report['test_index_started']=(runtime/'INDEX_STARTED').exists();assert report['test_index_started'] != args.preview
  if not args.complete_after_close:
   assert not (d.profile/'runtime/ottrin/index-v1.sock').exists();report['index_never_became_ready']=True
  assert (fixture/'original.txt').read_text()=='preserve original\n';report['original_bytes_preserved']=True
finally:
 report['teardown']=json.loads((out/'TEARDOWN.json').read_text()) if (out/'TEARDOWN.json').exists() else None
 report['source_binary_after']=identity(source);assert report['source_binary_after']==before
 (out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
