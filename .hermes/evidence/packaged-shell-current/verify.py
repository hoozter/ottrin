from pathlib import Path
import hashlib,importlib.util,json,os,signal,subprocess,sys
r=Path.cwd();e=Path(__file__).resolve().parent
s=importlib.util.spec_from_file_location('workspace',r/'scripts/test-workspace.py');w=importlib.util.module_from_spec(s);s.loader.exec_module(w)
for signum in [signal.SIGTERM,signal.SIGINT]:signal.signal(signum,w.cancelled)
paths=['Cargo.toml','Cargo.lock','crates/ottrin-platform/Cargo.toml','crates/ottrin-platform/src/windows_folder_integration.rs','crates/ottrin-ui/src/lib.rs','packaging/windows/msix-manifest.xml',*[str(p) for p in sorted((r/'crates/ottrin-shell').rglob('*')) if p.is_file()]]
def digests():return {str(Path(p).relative_to(r) if Path(p).is_absolute() else p):hashlib.sha256((r/p).read_bytes()).hexdigest() for p in paths}
state={'status':'running','base_revision':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'source_files':digests(),'completed':[]};child=None
commands=[('format.log',['cargo','fmt','--all','--check']),('windows-lint-final.log',['cargo','clippy','--locked','-p','ottrin-shell','-p','ottrin-platform','-p','ottrin-ui','--all-targets','--target','x86_64-pc-windows-gnu','--','-D','warnings']),('native-test-build.jsonl',['cargo','test','--locked','-p','ottrin-shell','--lib','--target','x86_64-pc-windows-gnu','--no-run','--message-format=json'])]
def save():(e/'VERIFY_STATE.json').write_text(json.dumps(state,indent=2)+'\n')
try:
 for name,command in commands:
  assert digests()==state['source_files'],'Source changed during verification'
  state.update(phase=name,command=command);save()
  with (e/name).open('w') as log:
   child=subprocess.Popen(command,env={**os.environ,'CARGO_BUILD_JOBS':'1'},stdout=log,stderr=subprocess.STDOUT,start_new_session=True);state['producer_pid']=child.pid;save()
   try:code=child.wait();assert code==0,(name,code)
   finally:w.stop_tests(child)
  assert digests()==state['source_files'],'Source changed during verification'
  state['completed'].append(name);print('Passed '+name,flush=True)
 state.update(status='pass format, strict Windows all-target lint and native-test compilation; runtime acceptance separate')
except BaseException as error:state.update(status='failed',error=repr(error));raise
finally:
 if child:w.stop_tests(child)
 state['producer_exit']=child.poll() if child else None;save()
