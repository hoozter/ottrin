"""Own two finite native theme checks; teardown precedes profile removal."""
from pathlib import Path
import hashlib,importlib.util,json,subprocess
root=Path.cwd();e=Path(__file__).resolve().parent
build=json.loads((e/'BUILD_SOURCE.json').read_text());result=json.loads((e/'BUILD_RESULT.json').read_text())
assert result['child_exit']==0
image=root/'dist/packages/Ottrin-0.1.0-x86_64.AppImage';digest=hashlib.file_digest(image.open('rb'),'sha256').hexdigest();assert result['artifacts'][image.name]==digest
spec=importlib.util.spec_from_file_location('desktop',root/'scripts/linux-desktop-qa/session.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
for mode,scale in [('Dark','1.5')]:
 out=e/('linux-pointer-'+mode.lower());out.mkdir(exist_ok=False)
 report={'status':'running','source_revision':build['source_revision'],'artifact_sha256':digest,'theme':mode,'scale':scale,'scope':'Mounted AppImage, private software X11, built-in Nord dark/light theme cloned through its actual Customize control; actual ordinary-pointer shared preview row/tab/window control actions and original complete-window captures. Full theme/platform/physical acceptance remains open.'}
 try:
  with m.OwnedDesktop(out,scale=scale) as desktop:
   config=json.loads((root/'.hermes/evidence/linux-cli-options-package-current/after100/TEARDOWN.json').read_text())['persisted_config']
   pref=desktop.profile/'config/ottrin/config.json';pref.parent.mkdir(parents=True);pref.write_text(json.dumps(config)+'\n')
   desktop.env.pop('NO_AT_BRIDGE',None);m.run(['gsettings','set','org.gnome.desktop.interface','toolkit-accessibility','true'],desktop.env)
   (out/'WORKER_SOURCE.py').write_bytes((e/'theme-pointer-worker.py').read_bytes());report['worker_sha256']=hashlib.sha256((out/'WORKER_SOURCE.py').read_bytes()).hexdigest()
   worker=desktop.spawn(['/usr/bin/python3',str(out/'WORKER_SOURCE.py'),str(image),str(out),mode],'worker.log')
   assert worker.wait(timeout=180)==0,'Native theme workflow failed; inspect FUNCTIONAL_RESULT.json and worker.log'
   report['status']='functional pass; original screenshots await direct visual review'
 except BaseException as error:
  report.update(status='failed',error=repr(error));raise
 finally:
  if (out/'TEARDOWN.json').exists():report['teardown']=json.loads((out/'TEARDOWN.json').read_text())
  (out/'CONTROL.json').write_text(json.dumps(report,indent=2)+'\n')
