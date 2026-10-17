from pathlib import Path
import json,sys
p=Path(__file__).resolve().parent;sys.path.insert(0,str(p));import transport as n
import os
p=Path(os.environ.get('OTTRIN_NATIVE_EVIDENCE_DIR',str(p)))
if len(sys.argv)>1:pid=int((p/'observer.pid').read_text())
else:
 pid=n.start(r"$missing=0;while(-not(Test-Path C:\OttrinCopyCleanupQA\results\RESULTS.json)){if((Get-ScheduledTask 'Ottrin Copy Cleanup QA').State.ToString()-in@('Running','Queued')){$missing=0}else{$missing++;if($missing-ge20){throw ('Owned task stopped without a result file: '+(Get-ScheduledTaskInfo 'Ottrin Copy Cleanup QA').LastTaskResult)}};Start-Sleep -Milliseconds 500};while((Get-ScheduledTask 'Ottrin Copy Cleanup QA').State-eq'Running'){Start-Sleep -Milliseconds 500};$r=(Get-ScheduledTaskInfo 'Ottrin Copy Cleanup QA').LastTaskResult;if($r-ne0){throw ('Actor failed: '+$r)}")
 (p/'observer.pid').write_text(str(pid))
try:n.wait(pid,45)
except TimeoutError as error:
 print(str(error),flush=True);raise SystemExit(75)
except AssertionError as error:
 for name in ['platform.stdout','platform.stderr','RESULTS.json']:
  try:n.download('results\\'+name,p/name)
  except Exception as download_error:
   if name!='RESULTS.json':raise
   (p/name).write_text(json.dumps({'status':'observation_failed','error':str(error),'result_download_error':str(download_error)},indent=2)+'\n')
 raise
for name in ['RESULTS.json','platform.stdout','platform.stderr']:n.download('results\\'+name,p/name)
