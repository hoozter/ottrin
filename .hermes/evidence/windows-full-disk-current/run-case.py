from pathlib import Path
import argparse,json,os,shutil,subprocess,sys
parser=argparse.ArgumentParser();parser.add_argument('--evidence-dir',type=Path,required=True);args=parser.parse_args()
root=Path.cwd();p=args.evidence_dir.resolve();runner=root/'.hermes/evidence/windows-copy-cleanup-current'
owner=json.loads((p/'OWNERSHIP.json').read_text());volume=json.loads((p/'IO_VOLUME.json').read_text(encoding='utf-8-sig'));assert owner['domain']=='ottrin-copy-cleanup-qa' and owner['token']==volume['token']
for index,(mode,leaf) in enumerate([('paired','windows_native_copy_disk_full_paired_journals_recover_after_space_is_freed'),('divergent','windows_native_copy_disk_full_divergent_journals_preserve_data_and_refuse_retry'),('destination-full','windows_native_copy_destination_full_with_separate_registry_recovers'),('registry-full','windows_native_copy_registry_full_with_separate_destination_preserves_and_refuses')]):
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
