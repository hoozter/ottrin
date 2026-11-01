"""Owned real-window Properties checks; preserve fixtures and preferences."""
from pathlib import Path
import importlib.util,json,hashlib,os,time
r=Path.cwd();e=Path(__file__).resolve().parent;b=r/'target/debug/ottrin-app'
spec=importlib.util.spec_from_file_location('desktop',r/'scripts/linux-desktop-qa/session.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
report={'binary_sha256':hashlib.sha256(b.read_bytes()).hexdigest(),'cases':[]}
for theme,scale in [('Dark','1'),('Light','1.5')]:
 out=e/('gui-final-'+theme.lower())
 with m.OwnedDesktop(out,scale=scale) as d:
  folder=d.profile/'fixture';folder.mkdir();source=folder/'report.pdf';source.write_bytes(b'Properties must not invoke a preview renderer or change this file.\n');link=folder/os.fsdecode(b'broken\x80');link.symlink_to(os.fsdecode(b'missing\x81'))
  config=json.loads((r/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config'];config.pop('restore_session',None);config.pop('last_session_dirs',None);config['theme']=theme
  cp=d.profile/'config/ottrin/config.json';cp.parent.mkdir();cp.write_text(json.dumps(config)+'\n');before=cp.read_bytes()
  for name,path in [('file',source),('link',link)]:
   p=d.spawn([str(b),'--properties','--',str(path)],name+'.log');deadline=time.monotonic()+25;windows=[]
   while not windows:
    assert p.poll() is None,(out/(name+'.log')).read_text();assert time.monotonic()<deadline
    windows=m.run(['xdotool','search','--onlyvisible','--pid',str(p.pid),'--name','Properties'],d.env,False).stdout.split();time.sleep(.1)
   w=windows[-1];assert int(m.run(['xdotool','getwindowpid',w],d.env).stdout)==p.pid
   assert (Path('/proc')/str(p.pid)/'exe').resolve()==b.resolve();time.sleep(.7)
   m.run(['import','-window',w,str(out/(name+'.png'))],d.env)
   m.run(['xdotool','windowactivate','--sync',w,'key','Escape'],d.env);p.wait(timeout=10);assert p.returncode==0
  assert cp.read_bytes()==before;assert source.read_bytes()==b'Properties must not invoke a preview renderer or change this file.\n';assert os.readlink(os.fsencode(link))==b'missing\x81'
  assert not list((d.profile/'runtime').rglob('index*.sock'))
  report['cases'].append({'theme':theme,'scale':scale,'normal_close':True,'preferences_and_source_preserved':True,'index_not_started':True})
 report['cases'][-1]['teardown']=json.loads((out/'TEARDOWN.json').read_text())
report['status']='pass';(e/'GUI_RESULT.json').write_text(json.dumps(report,indent=2)+'\n')
