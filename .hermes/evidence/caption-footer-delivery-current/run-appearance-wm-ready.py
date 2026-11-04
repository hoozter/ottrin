"""Actual AppImage/FUSE browser and cooperating-picker review in an owned guest."""
from pathlib import Path
import hashlib,json,signal,subprocess
R=Path.cwd();E=Path(__file__).resolve().parent;N=E/'appearance-wm-ready';child=None
build=json.loads((E/'BUILD_RESULT.json').read_text());content=json.loads((E/'PACKAGE_CONTENT_ACCEPTANCE.json').read_text())
assert build['status'].startswith('four-format build pass') and content['status']=='pass'
assert build['source_revision']==content['source_revision']
image=Path(build['stage'])/'Ottrin-0.1.0-x86_64.AppImage'
assert hashlib.file_digest(image.open('rb'),'sha256').hexdigest()==build['artifacts'][image.name]
(N/'BUILD_RESULT.json').write_text(json.dumps(build,indent=2)+'\n')
(N/'APPIMAGE_INPUT.json').write_text(json.dumps({'name':image.name,'source_revision':build['source_revision'],'sha256':build['artifacts'][image.name],'gui_sha256':content['runtime_sha256']['application']},indent=2)+'\n')
report={'status':'starting','source_revision':build['source_revision'],'artifact_sha256':build['artifacts'][image.name]}
def cancelled(*_):raise KeyboardInterrupt('Owned packaged Grid QA cancelled')
for sig in (signal.SIGTERM,signal.SIGINT):signal.signal(sig,cancelled)
try:
 command=['python3',str(R/'.hermes/evidence/recovery-transport-delivery-current/replacement/controller.py'),'ubuntu','--output',str(N),'--actor',str(N/'actor.py'),'--gui-qa']
 for p in [image,N/'APPIMAGE_INPUT.json',N/'worker.py',N/'linux-desktop-qa.py']:command+=['--input',str(p)]
 with (E/'appearance-wm-ready-controller.log').open('w') as log:
  child=subprocess.Popen(command,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
  code=child.wait();assert code==0,code
 report['status']='native fixture passed; original captures require direct review'
except BaseException as error:report.update(status='failed',error=repr(error));raise
finally:
 if child is not None and child.poll() is None:
  child.send_signal(signal.SIGTERM);child.wait(timeout=180)
 report['child_exit']=child.poll() if child else None
 report['no_host_input_alias_allocated']=True
 (E/'APPEARANCE_WM_READY_CONTROLLER.json').write_text(json.dumps(report,indent=2)+'\n')
