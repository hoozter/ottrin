"""Actual file-manager adoption QA, on a private desktop with a finite lease."""
from pathlib import Path
import argparse,hashlib,importlib.util,json,subprocess,time
r=Path.cwd();e=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');desktop=importlib.util.module_from_spec(spec);spec.loader.exec_module(desktop)
parser=argparse.ArgumentParser();parser.add_argument('--case',choices=['dark','light150','openwith'],required=True);args=parser.parse_args();out=e/args.case;assert not out.exists()
binary=r/'target/debug/ottrin-app';assert binary.is_file()
def sha(path):
 with path.open('rb') as source:return hashlib.file_digest(source,'sha256').hexdigest()
report={'case':args.case,'binary_sha256':sha(binary),'scope':'Actual source-debug file-manager/internal-picker GUI; not package or final resource acceptance'}
try:
 with desktop.OwnedDesktop(out,scale='1.5' if args.case=='light150' else '1') as d:
  fixture=d.profile/'fixture';fixture.mkdir();(fixture/'chosen').mkdir();(fixture/'exclude').mkdir();(fixture/'alpha.txt').write_text('alpha original\n')
  if args.case=='openwith':
   collector=fixture/'chosen-app';collector.write_text('#!/usr/bin/python3\nfrom pathlib import Path\nimport os,sys\nassert len(sys.argv)==2\nPath(__file__).with_name("opened-path.txt").write_bytes(os.fsencode(sys.argv[1]))\n');collector.chmod(0o700)
  initial={str(p.relative_to(fixture)):sha(p) for p in fixture.rglob('*') if p.is_file()};report['initial_files']=initial
  config=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];config.update(theme='Light' if args.case=='light150' else 'Dark',restore_session=False,last_session_dirs=[],window_size=[1100,760],installed_office_previews_enabled=False);config['bookmarks']=[];config['search'].update(include_roots=[str(fixture)],content_indexing_enabled=False)
  path=d.profile/'config/ottrin/config.json';path.parent.mkdir(parents=True);path.write_text(json.dumps(config)+'\n');(out/'CONFIG_BEFORE.json').write_text(json.dumps(config,indent=2)+'\n')
  app=d.spawn([str(binary),str(fixture)],'app.log')
  deadline=time.monotonic()+30
  while True:
   if app.poll() is not None:raise RuntimeError('Application exited during startup: '+str(app.returncode))
   q=desktop.run(['xdotool','search','--all','--onlyvisible','--pid',str(app.pid),'--name','Ottrin'],d.env,False);windows=q.stdout.strip().splitlines()
   if windows:break
   assert time.monotonic()<deadline,'Main window readiness timeout';time.sleep(.05)
  window=windows[-1];desktop.run(['xdotool','windowactivate','--sync',window],d.env);time.sleep(.7);desktop.run(['import','-window',window,str(out/'initial.png')],d.env)
  data={'profile':str(d.profile),'fixture':str(fixture),'app_pid':app.pid,'app_start_time':Path(f'/proc/{app.pid}/stat').read_text().split(') ',1)[1].split()[19],'window':window,'environment':{k:d.env[k] for k in ['DISPLAY','DBUS_SESSION_BUS_ADDRESS','HOME','XDG_CONFIG_HOME','XDG_CACHE_HOME','XDG_DATA_HOME','XDG_STATE_HOME','XDG_RUNTIME_DIR']}}
  (out/'SESSION.json').write_text(json.dumps(data,indent=2)+'\n')
  deadline=time.monotonic()+600
  while app.poll() is None and not (out/'STOP').exists():
   assert time.monotonic()<deadline,'Owned internal-picker QA lease expired';time.sleep(.2)
  report['normal_exit']=app.poll();report['final_files']={str(p.relative_to(fixture)):sha(p) for p in fixture.rglob('*') if p.is_file()}
  assert all(report['final_files'].get(name)==digest for name,digest in initial.items()),'Original fixture data changed'
  assert set(report['final_files'])-set(initial)<= ({'opened-path.txt'} if args.case=='openwith' else {'exported-theme.json'}),'Undeclared created file'
  if args.case=='openwith' and (fixture/'opened-path.txt').exists():
   assert (fixture/'opened-path.txt').read_bytes()==bytes(str(fixture/'alpha.txt'),'utf-8');report['openwith_exact_argument_verified']=True
  if (fixture/'exported-theme.json').exists():
   exported=json.loads((fixture/'exported-theme.json').read_text());assert exported['format']=='ottrin-theme' and exported['version']==2;report['exported_theme_name']=exported['theme']['name']
  report['config_after']=json.loads(path.read_text())
  prefs=path.with_name('picker.json');report['picker_preferences']=json.loads(prefs.read_text()) if prefs.exists() else None
  report['status']='normal exit and original bytes unchanged' if app.poll()==0 else 'stopped; no GUI acceptance'
finally:
 report['teardown']=json.loads((out/'TEARDOWN.json').read_text()) if (out/'TEARDOWN.json').exists() else None
 (out/'RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
