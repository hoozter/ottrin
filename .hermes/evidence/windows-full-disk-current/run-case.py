from pathlib import Path
import os,shutil,subprocess,sys
root=Path.cwd();p=Path(__file__).resolve().parent;runner=root/'.hermes/evidence/windows-copy-cleanup-current'
for index,(mode,leaf) in enumerate([('paired','windows_native_copy_disk_full_paired_journals_recover_after_space_is_freed'),('divergent','windows_native_copy_disk_full_divergent_journals_preserve_data_and_refuse_retry')]):
 case=p/mode;case.mkdir(exist_ok=True);shutil.copy2(p/'build-final.jsonl',case/'build.jsonl')
 name='copy_transaction::windows::journal::tests::disk_full::'+leaf
 args=[sys.executable,str(runner/'run-native.py'),'--evidence-dir',str(case),'--exact',name]
 if index:args+=['--reuse-binary']
 with (case/'dispatch.log').open('w') as output:subprocess.run(args,check=True,stdout=output,stderr=subprocess.STDOUT)
 env=os.environ.copy();env['OTTRIN_NATIVE_EVIDENCE_DIR']=str(case)
 with (case/'observe.log').open('w') as output:
  result=subprocess.run([sys.executable,str(runner/'observe.py')],env=env,stdout=output,stderr=subprocess.STDOUT)
  while result.returncode==75:result=subprocess.run([sys.executable,str(runner/'observe.py'),'resume'],env=env,stdout=output,stderr=subprocess.STDOUT)
  if result.returncode:raise RuntimeError('Native disk-full case failed: '+mode+'; inspect its result before any retry')
 print('Native disk-full case passed:',mode,flush=True)
