from pathlib import Path
import hashlib,json,os,re,shutil,sys,tempfile
p=Path(__file__).resolve().parent
sys.path.insert(0,str(p))
import transport as n
p=Path(os.environ.get('OTTRIN_NATIVE_EVIDENCE_DIR',str(p)))
dispatch=json.loads((p/'DISPATCH.json').read_text())
if (not isinstance(dispatch.get('id'),str) or not re.fullmatch(r'[0-9a-f]{32}',dispatch['id'])
 or not isinstance(dispatch.get('transferred_sha256'),str) or not re.fullmatch(r'[0-9a-f]{64}',dispatch['transferred_sha256'])
 or type(dispatch.get('watchdog_ms')) is not int or not 0<dispatch['watchdog_ms']<=900000
 or not isinstance(dispatch.get('test_arguments'),str)):
 raise RuntimeError('Invalid dispatch receipt')
if dispatch['status']!='dispatched':
 raise RuntimeError('Dispatch controller has not completed; wait for it before observing')
if len(sys.argv)>1:
 observer=json.loads((p/'OBSERVER.json').read_text())
 if observer['dispatch_id']!=dispatch['id']:
  raise RuntimeError('Observer belongs to another dispatch; start the current observer')
 pid=observer['guest_pid']
 if type(pid) is not int or not 0<pid<2**32:raise RuntimeError('Invalid guest observer identity')
else:
 old=p/'OBSERVER.json'
 if old.exists() and json.loads(old.read_text())['dispatch_id']==dispatch['id']:
  raise RuntimeError('Observer already exists for this dispatch; use resume')
 script=r"""$missing=0;$deadline=(Get-Date).AddMilliseconds(DEADLINE)
while($true){
 if((Get-Date)-gt$deadline){throw 'Observer deadline expired; inspect the native task before any restart'}
 $result=$null
 if(Test-Path C:\OttrinCopyCleanupQA\results\RESULTS.json){
  $candidate=Get-Content C:\OttrinCopyCleanupQA\results\RESULTS.json -Raw|ConvertFrom-Json
  if($candidate.dispatch_id-eq'TOKEN'){$result=$candidate}
 }
 $state=(Get-ScheduledTask 'Ottrin Copy Cleanup QA').State.ToString()
 if($null-ne$result-and$state-notin@('Running','Queued')){break}
 if($state-in@('Running','Queued')){$missing=0}else{$missing++;if($missing-ge20){throw 'Owned task stopped without its current dispatch result'}}
 Start-Sleep -Milliseconds 500
}
$r=(Get-ScheduledTaskInfo 'Ottrin Copy Cleanup QA').LastTaskResult
if($r-ne0){throw ('Actor failed: '+$r)}
""".replace('TOKEN',dispatch['id']).replace('DEADLINE',str(dispatch['watchdog_ms']+30000))
 pid=n.start(script)
 observer={'dispatch_id':dispatch['id'],'guest_pid':pid,'status':'waiting'}
 (p/'OBSERVER.json').write_text(json.dumps(observer,indent=2)+'\n')
 (p/'observer.pid').unlink(missing_ok=True)
wait_error=None
try:n.wait(pid,45)
except TimeoutError as error:
 print(str(error),flush=True)
 raise SystemExit(75)
except AssertionError as error:wait_error=error
# Never publish a stale result or logs from another run into accepted evidence.
try:
 with tempfile.TemporaryDirectory(prefix='.observation-',dir=p) as temporary:
  staging=Path(temporary)
  n.download(r'results\RESULTS.json',staging/'RESULTS.json')
  result=json.loads((staging/'RESULTS.json').read_text())
  if result.get('dispatch_id')!=dispatch['id'] or result.get('test_arguments')!=dispatch['test_arguments']:
   raise RuntimeError('Native result does not belong to this dispatch')
  if (result.get('status')=='pass' or result.get('test_binary_hash') is not None) and result.get('test_binary_hash')!=dispatch['transferred_sha256']:
   raise RuntimeError('Native result executable does not match dispatch')
  names=['RESULTS.json']
  for name in ['stdout','stderr']:
   digest=result.get(name+'_sha256')
   if digest is None:
    if result.get('status')=='pass':raise RuntimeError('Passing result lacks log hashes')
    continue
   target='platform.'+name
   n.download('results\\'+target,staging/target)
   if hashlib.sha256((staging/target).read_bytes()).hexdigest()!=digest:
    raise RuntimeError('Native log does not match dispatch result: '+target)
   names.append(target)
  if result.get('status')=='pass' and '--list' not in dispatch['test_arguments'].split():
   summaries=re.findall(rb'test result: ok\. (\d+) passed;', (staging/'platform.stdout').read_bytes())
   if not summaries or int(summaries[-1])==0:raise RuntimeError('Native dispatch executed no passing tests')
  for name in names:shutil.copyfile(staging/name,p/name)
except Exception as error:
 observer['status']='rejected';observer['error']=str(error)
 (p/'OBSERVER.json').write_text(json.dumps(observer,indent=2)+'\n')
 raise
observer['status']='complete' if wait_error is None and result.get('status')=='pass' else 'failed';(p/'OBSERVER.json').write_text(json.dumps(observer,indent=2)+'\n')
if wait_error is not None:raise wait_error
if result.get('status')!='pass':raise RuntimeError('Native actor failed: '+str(result.get('error') or result.get('artifact_error') or result.get('teardown_error') or 'unknown error'))
