"""Retire verified same-configuration replaced app/core test executables only."""
from pathlib import Path
import json,subprocess,hashlib,shutil
r=Path.cwd();e=Path(__file__).resolve().parent
assert '48 passed; 0 failed;' in (e/'APP_TESTS.log').read_text()
assert '43 passed; 0 failed;' in (e/'CORE_TESTS.log').read_text()
for p in Path('/proc').iterdir():
 if p.name.isdigit():
  try:assert (p/'comm').read_text().strip() not in {'cargo','rustc','rust-analyzer'}
  except (FileNotFoundError,ProcessLookupError):pass
plans=[]
for crate,old,new in [('ottrin-app','46397fa932c1de1b','ce54c356b815fc3f'),('ottrin-core','f537903df95abb79','b646676bf4cf13fc'),('ottrin-core','4e27adbf04cdf757','b646676bf4cf13fc')]:
 base=r/'target/debug';a=base/'.fingerprint'/f'{crate}-{old}';b=base/'.fingerprint'/f'{crate}-{new}'
 x=list(a.glob('test-*.json'));y=list(b.glob('test-*.json'));assert len(x)==len(y)==1
 previous=json.loads(x[0].read_text());current=json.loads(y[0].read_text());keys=['rustc','features','target','profile','rustflags','compile_kind'];assert all(previous[k]==current[k] for k in keys)
 refs=subprocess.run(['rg','-l',old,str(base/'.fingerprint'),'--glob','*.json'],text=True,capture_output=True);assert refs.returncode==0 and {Path(p) for p in refs.stdout.splitlines()}=={x[0]}
 exe=base/'deps'/f'{crate.replace("-","_")}-{old}';replacement=base/'deps'/f'{crate.replace("-","_")}-{new}';assert replacement.is_file()
 files=[exe,exe.with_name(exe.name+'.d'),*a.iterdir()]
 for p in files:assert p.is_file() and not p.is_symlink() and p.stat().st_nlink==1,p
 used=subprocess.run(['fuser',*[str(p) for p in files]],text=True,capture_output=True);assert used.returncode==1
 with exe.open('rb') as stream:digest=hashlib.file_digest(stream,'sha256').hexdigest()
 plans.append({'old':str(exe),'replacement':str(replacement),'sha256':digest,'same_compiler_settings':{k:previous[k] for k in keys},'bytes':sum(p.stat().st_size for p in files),'files':files,'fingerprint':a})
(e/'REPLACED_TEST_PLAN.json').write_text(json.dumps([{k:v for k,v in p.items() if k not in {'files','fingerprint'}} for p in plans],indent=2)+'\n')
for plan in plans:
 for p in plan['files']:p.unlink()
 plan['fingerprint'].rmdir()
(e/'REPLACED_TEST_CLEANUP.json').write_text(json.dumps({'executables_retired':len(plans),'logical_bytes':sum(p['bytes'] for p in plans),'current_binaries_and_dependencies_preserved':True},indent=2)+'\n')
