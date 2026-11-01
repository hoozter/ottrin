"""Retire the exact superseded Windows UI test after a matching complete native replacement."""
from pathlib import Path
import hashlib,json,shutil,subprocess
r=Path.cwd();e=Path(__file__).resolve().parent
assert json.loads((e/'platform-final/RUNTIME_OWNER.json').read_text())['status']=='pass'
for role in ['ordinary','elevated']:
 p=e/('short-temp-'+role+'-ui-runtime.stdout')
 assert 'test result: ok. 409 passed; 0 failed; 4 ignored; 0 measured; 0 filtered out;' in p.read_text()
records=[json.loads(line) for line in (e/'windows-final-build.jsonl').read_text().splitlines() if line.startswith('{')]
assert records[-1]=={'reason':'build-finished','success':True}
new=[Path(x['executable']) for x in records if x.get('executable') and x.get('target',{}).get('name')=='ottrin_ui'];assert len(new)==1
old=r/'target/x86_64-pc-windows-gnu/debug/deps/ottrin_ui-1c2af95a0e3b7fca.exe';new=new[0];assert old!=new and old.is_file() and new.is_file()
with new.open('rb') as f:assert hashlib.file_digest(f,'sha256').hexdigest()==json.loads((e/'RUNTIME_BINARIES.json').read_text())['fixture-ui.exe']['raw_sha256']
base=old.parent.parent;old_hash=old.stem.rsplit('-',1)[1];new_hash=new.stem.rsplit('-',1)[1];a=base/'.fingerprint'/('ottrin-ui-'+old_hash);b=base/'.fingerprint'/('ottrin-ui-'+new_hash)
x=list(a.glob('test-*.json'));y=list(b.glob('test-*.json'));assert len(x)==len(y)==1
old_cfg=json.loads(x[0].read_text());new_cfg=json.loads(y[0].read_text());keys=['rustc','features','target','profile','rustflags','compile_kind'];assert all(old_cfg[k]==new_cfg[k] for k in keys)
refs=subprocess.run(['rg','-l',old_hash,str(base/'.fingerprint'),'--glob','*.json'],capture_output=True,text=True);assert refs.returncode==0 and {Path(x) for x in refs.stdout.splitlines()}=={a/x[0].name}
paths=[old,old.with_suffix('.d'),*a.iterdir()]
assert all(p.is_file() and not p.is_symlink() and p.stat().st_nlink==1 for p in paths)
for p in Path('/proc').iterdir():
 if p.name.isdigit():
  try:assert (p/'comm').read_text().strip() not in {'cargo','rustc','rust-analyzer'}
  except (FileNotFoundError,ProcessLookupError):pass
assert subprocess.run(['fuser',str(old)],capture_output=True).returncode==1
size=sum(p.stat().st_size for p in paths);free=shutil.disk_usage(r).free
with old.open('rb') as f:digest=hashlib.file_digest(f,'sha256').hexdigest()
plan={'old':str(old),'replacement':str(new),'same_compiler_configuration':{k:old_cfg[k] for k in keys},'sha256':digest,'logical_bytes':size,'paths':[str(p) for p in paths]}
(e/'REPLACED_TEST_PLAN.json').write_text(json.dumps(plan,indent=2)+'\n')
old.unlink();old.with_suffix('.d').unlink();shutil.rmtree(a)
(e/'REPLACED_TEST_CLEANUP.json').write_text(json.dumps({'retired_artifacts':1,'logical_bytes':size,'filesystem_free_delta':shutil.disk_usage(r).free-free,'current_binaries_dependencies_and_test_replacements_preserved':True},indent=2)+'\n')
