"""Serial shared-cache checks and source GUI build with registered process teardown."""
from pathlib import Path
import hashlib,importlib.util,json,os,signal,subprocess,sys
sys.dont_write_bytecode=True
R=Path.cwd();E=Path(__file__).resolve().parent
s=importlib.util.spec_from_file_location('workspace',R/'scripts/test-workspace.py');w=importlib.util.module_from_spec(s);s.loader.exec_module(w)
for sig in [signal.SIGTERM,signal.SIGINT]:signal.signal(sig,w.cancelled)
assert not subprocess.check_output(['virsh','-c','qemu:///system','list','--name'],text=True).strip()
source=R/'crates/ottrin-ui/src/lib.rs';digest=hashlib.sha256(source.read_bytes()).hexdigest()
state={'status':'running','source_head':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'dirty_source_file_sha256':digest,'packaged':False,'completed':[]};child=None
commands=[('workspace-format.log',['cargo','fmt','--all','--check']),('ui-tests.log',[sys.executable,'scripts/test-workspace.py','-p','ottrin-ui','--lib','--','--test-threads=1']),('native-lint.log',['cargo','clippy','-p','ottrin-ui','--all-targets','--','-D','warnings']),('windows-lint.log',['cargo','clippy','-p','ottrin-ui','--all-targets','--target','x86_64-pc-windows-gnu','--','-D','warnings']),('gui-build.log',['cargo','build','--release','-p','ottrin-app','--bin','ottrin-app'])]
env={**os.environ,'CARGO_BUILD_JOBS':'1','PKG_CONFIG_PATH':str(R/'.hermes/tools/gstreamer-dev/usr/lib/x86_64-linux-gnu/pkgconfig')}
def save():(E/'VERIFY_STATE.json').write_text(json.dumps(state,indent=2)+'\n')
try:
 for log,command in commands:
  assert hashlib.sha256(source.read_bytes()).hexdigest()==digest
  state.update(phase=log,command=command);save()
  with (E/log).open('w') as output:
   child=subprocess.Popen(command,env=env,stdout=output,stderr=subprocess.STDOUT,start_new_session=True);state['producer_pid']=child.pid;save()
   try:code=child.wait();assert code==0,(log,code)
   finally:w.stop_tests(child)
  state['completed'].append(log);print('Passed '+log,flush=True)
 state.update(status='pass full UI tests, strict native/Windows lint and source GUI build; native layout verification pending',gui_sha256=hashlib.sha256((R/'target/release/ottrin-app').read_bytes()).hexdigest(),index_daemon_sha256=hashlib.sha256((R/'target/release/ottrin-indexd').read_bytes()).hexdigest())
except BaseException as error:state.update(status='failed; inspect phase log',error=repr(error));raise
finally:
 if child:w.stop_tests(child)
 state['producer_exit']=child.poll() if child else None;save()
