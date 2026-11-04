"""Run one owned GUI fixture; remove only its temporary executable input."""
from pathlib import Path
import hashlib,json,os,shutil,signal,subprocess
R=Path.cwd();E=Path(__file__).resolve().parent;N=E/'native';inputs=R/'.hermes/packaging-tmp/.grid-native-input';child=None;owned=False
assert not inputs.exists()
head=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()
assert head.startswith('7fefe191')
assert set(subprocess.check_output(['git','diff','--name-only','HEAD'],text=True).splitlines()) <= {'.hermes/evidence/release-audit-current/USER_REQUIREMENTS.json'}
checks=json.loads((E/'SOURCE_CHECKS.json').read_text())
for name,digest in checks['source_sha256'].items():
 assert hashlib.sha256((R/name).read_bytes()).hexdigest()==digest
 assert hashlib.sha256(subprocess.check_output(['git','show',head+':'+name])).hexdigest()==digest
report={'status':'starting','source_revision':head,'temporary_input':str(inputs)}
def cancelled(*_):raise KeyboardInterrupt('Owned Grid native QA cancelled')
for sig in (signal.SIGTERM,signal.SIGINT):signal.signal(sig,cancelled)
try:
 inputs.mkdir();owned=True
 gui=inputs/'grid-ottrin-app';shutil.copyfile(R/'target/release/ottrin-app',gui)
 digest=hashlib.file_digest(gui.open('rb'),'sha256').hexdigest()
 assert digest!='8569a35a2e182c504abd2171a2773316443a8356cea213f4fe60e4c406fcfa42','Build did not replace the preceding GUI'
 (N/'GUI_INPUT.json').write_text(json.dumps({'source_revision':head,'gui_sha256':digest,'scope':'New release GUI only, with separately identified DAC native package/service. Not a complete new package.'},indent=2)+'\n')
 controller=R/'.hermes/evidence/recovery-transport-delivery-current/replacement/controller.py'
 command=['python3',str(controller),'ubuntu','--output',str(N),'--actor',str(N/'actor.py'),'--gui-qa']
 for p in [gui,N/'GUI_INPUT.json',N/'worker.py',N/'linux-desktop-qa.py']:command+=['--input',str(p)]
 with (E/'native-controller.log').open('w') as log:
  child=subprocess.Popen(command,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
  code=child.wait();assert code==0,code
 report['status']='native fixture passed; original captures and observations require assessment'
except BaseException as error:
 report.update(status='failed',error=repr(error));raise
finally:
 if child is not None and child.poll() is None:
  child.send_signal(signal.SIGTERM);child.wait(timeout=180)
 report['child_exit']=child.poll() if child else None
 if owned:
  files=list(inputs.iterdir());assert all(p.is_file() and not p.is_symlink() and p.stat().st_nlink==1 for p in files)
  assert subprocess.run(['fuser',*map(str,files)],capture_output=True).returncode==1
  shutil.rmtree(inputs)
 report['owned_input_removed']=not inputs.exists()
 (E/'NATIVE_CONTROLLER.json').write_text(json.dumps(report,indent=2)+'\n')
