"""Check the owned draft at the normal crate path/cache, restoring the delivery source."""
from pathlib import Path
import hashlib,json,os,signal,subprocess
r=Path.cwd();e=Path(__file__).resolve().parent;source=r/'crates/ottrin-ui/src/lib.rs';draft=r/'.hermes/worktrees/theme-preview-presenters/crates/ottrin-ui/src/lib.rs'
original=source.read_bytes();candidate=draft.read_bytes()
assert original==subprocess.check_output(['git','show','0613162ddee19361e02eb9a3cc315d8a4e643493:crates/ottrin-ui/src/lib.rs'])
report={'source_before_sha256':hashlib.sha256(original).hexdigest(),'draft_sha256':hashlib.sha256(candidate).hexdigest(),'scope':'Draft UI tests at the existing crate path and compiler configuration; source restored before any delivery mutation','status':'running'}
def cancelled(*_):raise KeyboardInterrupt('Draft check cancelled')
for sig in [signal.SIGTERM,signal.SIGINT]:signal.signal(sig,cancelled)
process=None
try:
 source.write_bytes(candidate)
 env={**os.environ,'CARGO_BUILD_JOBS':'1','PKG_CONFIG_PATH':str(r/'.hermes/tools/gstreamer-dev/usr/lib/x86_64-linux-gnu/pkgconfig')}
 commands=[('ui-tests',['python3','scripts/test-workspace.py','-p','ottrin-ui','--lib']),('native-lint',['cargo','clippy','-p','ottrin-ui','--all-targets','--','-D','warnings']),('windows-lint',['cargo','clippy','--target','x86_64-pc-windows-gnu','-p','ottrin-ui','--all-targets','--','-D','warnings'])]
 report['checks']=[]
 for name,command in commands:
  with (e/(name+'.log')).open('w') as log:
   process=subprocess.Popen(command,env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
   code=process.wait()
  report['checks'].append({'name':name,'command':command,'exit':code})
  if code:break
 report['exit']=code;report['status']='pass source UI suite and strict native/Windows UI lint' if code==0 else 'fail source checks'

finally:
 if process is not None and process.poll() is None:
  os.killpg(process.pid,signal.SIGTERM);process.wait(timeout=15)
 assert source.read_bytes()==candidate,'Unexpected external source edit; retain original instead of overwriting it'
 source.write_bytes(original);report['delivery_source_restored']=source.read_bytes()==original
 (e/'SOURCE_CHECK.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report));raise SystemExit(report.get('exit',1))
