from pathlib import Path
import json,os,subprocess,signal,importlib.util
r=Path.cwd();e=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('workspace',r/'scripts/test-workspace.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
for sig in [signal.SIGTERM,signal.SIGINT]:signal.signal(sig,m.cancelled)
env={**os.environ,'CARGO_BUILD_JOBS':'1','PKG_CONFIG_PATH':str(r/'.hermes/tools/gstreamer-dev/usr/lib/x86_64-linux-gnu/pkgconfig')}
steps=[('full-search-tests',['python3','scripts/test-workspace.py','-p','ottrin-search','--lib']),('native-clippy',['cargo','clippy','-p','ottrin-search','--all-targets','--','-D','warnings']),('windows-clippy',['cargo','clippy','--target','x86_64-pc-windows-gnu','-p','ottrin-search','--all-targets','--','-D','warnings']),('format',['cargo','fmt','--all','--','--check'])]
report={'status':'running','scope':'Test-only bounded SQLiteBusy polling with deterministic real-lock regression and unchanged exact watcher outcomes; shipping runtime prefix unchanged','checks':[]}
try:
 for name,command in steps:
  with (e/(name+'.log')).open('w') as log:
   p=subprocess.Popen(command,env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
   try:code=p.wait()
   finally:m.stop_tests(p)
  report['checks'].append({'name':name,'command':command,'exit':code});(e/'SOURCE_CHECKS.json').write_text(json.dumps(report,indent=2)+'\n');assert code==0,name
 report['status']='pass full Search tests/native and Windows strict lint/format; test-only runtime-prefix proof separate'
except BaseException:
 report['status']='failed source check; inspect checks/logs';raise
finally:(e/'SOURCE_CHECKS.json').write_text(json.dumps(report,indent=2)+'\n')
