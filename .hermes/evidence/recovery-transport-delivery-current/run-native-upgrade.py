"""Run the owned real-login helper upgrade fixture and retire its host input."""
from pathlib import Path
import hashlib,json,os,shutil,signal,subprocess,tarfile
R=Path.cwd();E=Path(__file__).resolve().parent;N=E/'native-upgrade'
build=json.loads((E/'BUILD_RESULT.json').read_text());assert build['status'].startswith('four-format build pass')
assert json.loads((E/'PACKAGE_CONTENT_ACCEPTANCE.json').read_text())['source_revision']==build['source_revision']
assert not subprocess.check_output(['git','status','--porcelain','--untracked-files=no'],text=True).strip()
inputs=R/'.hermes/packaging-tmp/.helper-protocol-input';assert not inputs.exists()
child=None;owned=False
report={'status':'starting','input_directory':str(inputs),'source_revision':build['source_revision']}
def cancelled(*_):raise KeyboardInterrupt('Native upgrade controller cancelled')
for sig in (signal.SIGTERM,signal.SIGINT):signal.signal(sig,cancelled)
try:
 inputs.mkdir();owned=True
 old_archive=R/'dist/packages/Ottrin-0.1.0-linux-x86_64-portable.tar.gz'
 authority=json.loads((R/'dist/packages/LINUX_PACKAGES_ACCEPTANCE.json').read_text())
 assert authority['source_revision']=='aa2416330e9209c844f99e5a318e44b3c9a5c277'
 assert hashlib.sha256(old_archive.read_bytes()).hexdigest()==authority['artifacts'][old_archive.name]
 with tarfile.open(old_archive) as archive:
  members=[m for m in archive if m.name.endswith('/libexec/ottrin/ottrin-priv-helper')];assert len(members)==1
  old=inputs/'old-helper';old.write_bytes(archive.extractfile(members[0]).read());old.chmod(0o755)
 config={'sha256':hashlib.sha256(old.read_bytes()).hexdigest(),'archive_sha256':hashlib.sha256(old_archive.read_bytes()).hexdigest(),'scope':'Exact incompatible protocol2 helper from retained stable Linuxaa portable package'}
 (N/'QA_OLD_HELPER_INPUT.json').write_text(json.dumps(config,indent=2)+'\n')
 image=Path(build['stage'])/'Ottrin-0.1.0-x86_64.AppImage'
 (N/'QA_APPIMAGE_INPUT.json').write_text(json.dumps({'name':image.name,'sha256':build['artifacts'][image.name]},indent=2)+'\n')
 (N/'BUILD_RESULT.json').write_text(json.dumps(build,indent=2)+'\n')
 command=['python3',str(N/'controller.py'),'ubuntu','--output',str(N),'--actor',str(N/'actor.py'),'--gui-qa']
 for p in [N/'prompt-worker.py',N/'linux-desktop-qa.py',N/'QA_PROTECTED_INPUT.json',N/'QA_APPIMAGE_INPUT.json',N/'QA_OLD_HELPER_INPUT.json',image,old]:command+=['--input',str(p)]
 with (N/'controller.log').open('w') as log:
  child=subprocess.Popen(command,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
  code=child.wait();assert code==0,code
 report['status']='native upgrade controller passed; captures and detailed receipts require assessment'
except BaseException as error:
 report.update(status='failed',error=repr(error));raise
finally:
 try:
  if child is not None and child.poll() is None:
   child.send_signal(signal.SIGTERM)
   child.wait(timeout=180)
 except BaseException as cleanup_error:
  report.update(status='teardown pending; retain input and inspect owned VM',cleanup_error=repr(cleanup_error),live_controller_pid=child.pid)
  (E/'NATIVE_UPGRADE_CONTROLLER.json').write_text(json.dumps(report,indent=2)+'\n')
  raise
 report['child_exit']=child.poll() if child else None
 if owned:
  files=[p for p in inputs.iterdir() if p.is_file()]
  assert all(p.stat().st_nlink==1 for p in files)
  if files:assert subprocess.run(['fuser',*map(str,files)],capture_output=True).returncode==1
  shutil.rmtree(inputs)
 report['owned_host_input_removed']=not inputs.exists()
 (E/'NATIVE_UPGRADE_CONTROLLER.json').write_text(json.dumps(report,indent=2)+'\n')
