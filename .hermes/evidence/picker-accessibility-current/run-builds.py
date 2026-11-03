from pathlib import Path
import subprocess,signal,importlib.util,json,os
r=Path.cwd();e=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('workspace',r/'scripts/test-workspace.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
for s in [signal.SIGTERM,signal.SIGINT]:signal.signal(s,m.cancelled)
report={'status':'running','source_revision':json.loads((e/'BUILD_SOURCE.json').read_text())['source_revision'],'controller_pid':os.getpid(),'steps':[]}
def save():(e/'BUILD_PIPELINE.json').write_text(json.dumps(report,indent=2)+'\n')
save()
try:
 for name,script in [('linux',e/'build.py'),('windows',e/'windows/build-portable.py')]:
  report['active']=name;save();p=subprocess.Popen(['python3',str(script)],start_new_session=True)
  try:code=p.wait()
  finally:m.stop_tests(p)
  report['steps'].append({'name':name,'exit':code});save();assert code==0,(name,code)
 report['status']='both staged builds completed; exact content/native acceptance and promotion remain required'
except BaseException as error:report['status']='interrupted or failed; inspect per-stage owner receipts';report['error']=repr(error);raise
finally:save()
