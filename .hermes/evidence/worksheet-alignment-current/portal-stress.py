"""Repeat the real portal cancellation fixtures with shared cache/private state."""
from pathlib import Path
import importlib.util,json,os,signal,subprocess,time
r=Path.cwd();e=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('test_workspace',r/'scripts/test-workspace.py');workspace=importlib.util.module_from_spec(spec);spec.loader.exec_module(workspace)
for sig in [signal.SIGTERM,signal.SIGINT]:signal.signal(sig,workspace.cancelled)
executable=r/'target/debug/deps/ottrin_app-8443172d8770f5ec';assert executable.is_file();results=[]
try:
 with workspace.isolated_environment() as (profile,env):
  for iteration in range(20):
   with (e/'portal-stress.log').open('a') as log:
    log.write(f'Iteration {iteration}\n');log.flush();p=subprocess.Popen([str(executable),'portal::tests','--nocapture'],env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
    try:code=p.wait(timeout=20)
    finally:workspace.stop_tests(p)
    results.append({'iteration':iteration,'exit':code});assert code==0,('Portal fixture failed; inspect preserved diagnostic',iteration)
finally:
 (e/'PORTAL_STRESS.json').write_text(json.dumps({'iterations':results,'profile':str(profile),'profile_removed':not profile.exists(),'scope':'Local repeated actual cancellation tests; does not establish cause of the earlier hosted pre-lease failure'},indent=2)+'\n')
