from pathlib import Path
import json,os,shutil,subprocess,sys
root=Path.cwd();p=Path(__file__).resolve().parent;runner=root/'.hermes/evidence/windows-copy-cleanup-current'
sys.path.insert(0,str(runner));import transport as n
name='copy_transaction::windows::journal::tests::throttled_io::windows_native_copy_throttled_journal_io_stop_preserves_data_and_recovers'
for index,mode in enumerate(['forward','undo','cleanup']):
 case=p/mode;case.mkdir(exist_ok=True)
 shutil.copy2(p/'build-final.jsonl',case/'build.jsonl');shutil.copy2(p/'OWNERSHIP.json',case/'OWNERSHIP.json')
 pid=n.start(r"$v=Get-Content C:\OttrinCopyCleanupQA\IO_VOLUME.json -Raw|ConvertFrom-Json;if((Get-Content (Join-Path $v.root '.ottrin-owned-secondary-volume') -Raw).Trim()-ne$v.token){throw 'Volume token mismatch'};$t=Get-ScheduledTask 'Ottrin Copy Cleanup QA';if($t.State.ToString()-in@('Running','Queued')){throw 'Prior actor still live'};foreach($name in @('ready','go','released','cancel-observed')){$f=Join-Path $v.control $name;if(Test-Path $f){Remove-Item -LiteralPath $f -Force}}")
 n.wait(pid,30)
 test=name if mode=='forward' else name.replace('_io_stop','_'+mode+'_stop')
 args=[sys.executable,str(runner/'run-native.py'),'--evidence-dir',str(case),'--exact',test]
 if index:args+=['--reuse-binary']
 with (case/'dispatch.log').open('w') as output:subprocess.run(args,check=True,stdout=output,stderr=subprocess.STDOUT)
 env=os.environ.copy();env['OTTRIN_NATIVE_EVIDENCE_DIR']=str(case)
 with (case/'control.log').open('w') as output:subprocess.run([sys.executable,str(p/'control.py')],env=env,check=True,stdout=output,stderr=subprocess.STDOUT)
 with (case/'observe.log').open('w') as output:
  result=subprocess.run([sys.executable,str(runner/'observe.py')],env=env,stdout=output,stderr=subprocess.STDOUT)
  while result.returncode==75:result=subprocess.run([sys.executable,str(runner/'observe.py'),'resume'],env=env,stdout=output,stderr=subprocess.STDOUT)
  if result.returncode:raise RuntimeError('Native case failed: '+mode)
 print(mode,'pass',flush=True)

for mode,flag in [('oplock','--copy-oplock'),('guard','--copy-io-cancellation'),('start-barrier','--group-start'),('configured','--configured-actions'),('same-plan-stop','--same-plan-stop')]:
 case=p/mode;case.mkdir(exist_ok=True);shutil.copy2(p/'build-final.jsonl',case/'build.jsonl')
 args=[sys.executable,str(runner/'run-native.py'),'--evidence-dir',str(case),'--reuse-binary']
 if flag=='--same-plan-stop':args+=['--exact','copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cleanup_stop_after_control_write_keeps_same_plan_retryable']
 else:args+=[flag]
 with (case/'dispatch.log').open('w') as output:subprocess.run(args,check=True,stdout=output,stderr=subprocess.STDOUT)
 env=os.environ.copy();env['OTTRIN_NATIVE_EVIDENCE_DIR']=str(case)
 with (case/'observe.log').open('w') as output:
  result=subprocess.run([sys.executable,str(runner/'observe.py')],env=env,stdout=output,stderr=subprocess.STDOUT)
  while result.returncode==75:result=subprocess.run([sys.executable,str(runner/'observe.py'),'resume'],env=env,stdout=output,stderr=subprocess.STDOUT)
  if result.returncode:raise RuntimeError('Native regression failed: '+mode)
 print(mode,'pass',flush=True)
