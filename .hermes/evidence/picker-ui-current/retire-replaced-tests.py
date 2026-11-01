"""Retire only the two replaced test artifacts after complete replacement suites."""
from pathlib import Path
import hashlib,json,shutil,subprocess
root=Path.cwd();e=root/'.hermes/evidence/picker-ui-current'
assert json.loads((e/'RUNTIME_OWNER.json').read_text())['status']=='pass'
assert '422 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out;' in (e/'COMPLETE_NATIVE_LOCATION_FIXED.log').read_text()
for p in Path('/proc').iterdir():
 if p.name.isdigit():
  try:assert (p/'comm').read_text().strip() not in {'cargo','rustc','rust-analyzer'}
  except (FileNotFoundError,ProcessLookupError):pass
pairs=[('target/debug','9cf5f2c9763f2daf','21cb108f787ec80e',''),('target/x86_64-pc-windows-gnu/debug','ebf8e9137cceb236','fa30a86b4cca8a3a','.exe')]
plan=[]
for target,old,new,suffix in pairs:
 base=root/target;old_fp=base/'.fingerprint'/('ottrin-ui-'+old);new_fp=base/'.fingerprint'/('ottrin-ui-'+new)
 a=json.loads((old_fp/'test-lib-ottrin_ui.json').read_text());b=json.loads((new_fp/'test-lib-ottrin_ui.json').read_text())
 keys=['rustc','features','target','profile','rustflags','compile_kind'];assert all(a[k]==b[k] for k in keys)
 assert (base/'deps'/('ottrin_ui-'+new+suffix)).is_file()
 refs=subprocess.run(['rg','-l',old,str(base/'.fingerprint'),'--glob','*.json'],text=True,capture_output=True,check=False)
 assert refs.returncode==0 and {Path(p) for p in refs.stdout.splitlines()}=={old_fp/'test-lib-ottrin_ui.json'}
 binary=base/'deps'/('ottrin_ui-'+old+suffix);dep=base/'deps'/('ottrin_ui-'+old+'.d')
 for p in [binary,dep,*old_fp.iterdir()]:assert p.is_file() and not p.is_symlink() and p.stat().st_nlink==1,p
 consumer=subprocess.run(['fuser',str(binary)],text=True,capture_output=True,check=False);assert consumer.returncode==1,(binary,consumer.stdout,consumer.stderr)
 plan.append({'old':str(binary),'replacement':str(base/'deps'/('ottrin_ui-'+new+suffix)),'same_compiler_configuration':{k:a[k] for k in keys},'old_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'logical_bytes':sum(p.stat().st_size for p in [binary,dep,*old_fp.iterdir()]),'consumers':0,'hard_links':1,'paths':[binary,dep,old_fp]})
(e/'REPLACED_TEST_PLAN.json').write_text(json.dumps([{k:v for k,v in row.items() if k!='paths'} for row in plan],indent=2)+'\n')
before=shutil.disk_usage(root).free
for row in plan:
 for p in row['paths']:
  if p.is_dir():shutil.rmtree(p)
  else:p.unlink()
after=shutil.disk_usage(root).free
(e/'REPLACED_TEST_CLEANUP.json').write_text(json.dumps({'retired_artifacts':2,'logical_bytes':sum(row['logical_bytes'] for row in plan),'filesystem_free_delta':after-before,'current_tests_and_shared_dependencies_retained':True},indent=2)+'\n')
