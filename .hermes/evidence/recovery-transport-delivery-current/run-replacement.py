"""Own old-release input aliases and the registered native replacement fixture."""
from pathlib import Path
import hashlib,json,shutil,signal,subprocess,tarfile
R=Path.cwd();E=Path(__file__).resolve().parent;N=E/'replacement';inputs=R/'.hermes/packaging-tmp/.replacement-input'
assert not inputs.exists()
read=lambda p:json.loads(p.read_text())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
old=read(R/'dist/packages/LINUX_PACKAGES_ACCEPTANCE.json');new=read(E/'BUILD_RESULT.json');assert old['source_revision']=='aa2416330e9209c844f99e5a318e44b3c9a5c277'
portable='Ottrin-0.1.0-linux-x86_64-portable.tar.gz';image='Ottrin-0.1.0-x86_64.AppImage';stage=Path(new['stage']);child=None;owned=False
report={'status':'starting','source_revision':new['source_revision'],'input_directory':str(inputs)}
def cancelled(*_):raise KeyboardInterrupt('Owned replacement fixture cancelled')
for sig in (signal.SIGTERM,signal.SIGINT):signal.signal(sig,cancelled)
try:
 inputs.mkdir();owned=True;data={'portable_root':'Ottrin-0.1.0-linux-x86_64'}
 for label,root,manifest in [('old',R/'dist/packages',old),('new',stage,new)]:
  assert sha(root/portable)==manifest['artifacts'][portable] and sha(root/image)==manifest['artifacts'][image]
  data[label]={'source_revision':manifest['source_revision'],'portable_sha256':manifest['artifacts'][portable],'appimage_sha256':manifest['artifacts'][image]}
  with tarfile.open(root/portable) as archive:
   for key,path in [('gui_sha256','libexec/ottrin/ottrin'),('daemon_sha256','libexec/ottrin/ottrin-indexd')]:
    member=archive.getmember(data['portable_root']+'/'+path);data[label][key]=hashlib.file_digest(archive.extractfile(member),'sha256').hexdigest()
 assert data['old']['daemon_sha256']!=data['new']['daemon_sha256'],'Require an actual daemon binary replacement'
 shutil.copyfile(R/'dist/packages'/image,inputs/'old.AppImage');shutil.copyfile(R/'dist/packages'/portable,inputs/'old-portable.tar.gz')
 (N/'REPLACEMENT_INPUT.json').write_text(json.dumps(data,indent=2)+'\n')
 command=['python3',str(N/'controller.py'),'ubuntu','--output',str(N),'--actor',str(N/'actor.py'),'--gui-qa']
 for path in [N/'worker.py',E/'native-upgrade/linux-desktop-qa.py',N/'REPLACEMENT_INPUT.json',stage/image,inputs/'old.AppImage',inputs/'old-portable.tar.gz']:command+=['--input',str(path)]
 with (N/'controller.log').open('w') as log:
  child=subprocess.Popen(command,stdout=log,stderr=subprocess.STDOUT,start_new_session=True);code=child.wait();assert code==0,code
 report['status']='replacement controller passed; receipts/captures require assessment'
except BaseException as error:report.update(status='failed',error=repr(error));raise
finally:
 try:
  if child is not None and child.poll() is None:child.send_signal(signal.SIGTERM);child.wait(timeout=180)
 except BaseException as cleanup_error:
  report.update(status='teardown pending; retain input and inspect owned VM',cleanup_error=repr(cleanup_error),live_controller_pid=child.pid)
  (E/'REPLACEMENT_CONTROLLER.json').write_text(json.dumps(report,indent=2)+'\n');raise
 report['child_exit']=child.poll() if child else None
 if owned:
  files=list(inputs.iterdir());assert all(p.is_file() and not p.is_symlink() and p.stat().st_nlink==1 for p in files)
  if files:assert subprocess.run(['fuser',*map(str,files)],capture_output=True).returncode==1
  shutil.rmtree(inputs)
 report['owned_host_input_removed']=not inputs.exists()
 (E/'REPLACEMENT_CONTROLLER.json').write_text(json.dumps(report,indent=2)+'\n')
