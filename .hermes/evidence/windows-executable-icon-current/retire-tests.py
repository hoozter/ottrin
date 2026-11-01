"""Retire exact executable replacements after complete native/app execution."""
from pathlib import Path
import json,subprocess,hashlib,shutil
r=Path.cwd();e=r/'.hermes/evidence/windows-executable-icon-current'
assert json.loads((e/'native-final/RUNTIME_OWNER.json').read_text())['status']=='pass'
for role in ['ordinary','elevated']:assert '32 passed; 0 failed;' in (e/f'native-final/short-temp-{role}-app-runtime.stdout').read_text()
for p in Path('/proc').iterdir():
 if p.name.isdigit():
  try:assert (p/'comm').read_text().strip() not in {'cargo','rustc','rust-analyzer'}
  except (FileNotFoundError,ProcessLookupError):pass
plans=[]
for row in json.loads((e/'REPLACED_TEST_PAIRS.json').read_text()):
 old=Path(row['old']);new=Path(row['new']);assert old.is_file() and new.is_file() and old!=new
 base=old.parent.parent;oh=old.stem.rsplit('-',1)[1];nh=new.stem.rsplit('-',1)[1]
 a=base/'.fingerprint'/('ottrin-app-'+oh);b=base/'.fingerprint'/('ottrin-app-'+nh)
 x=list(a.glob('test-*.json'));y=list(b.glob('test-*.json'));assert len(x)==len(y)==1
 previous=json.loads(x[0].read_text());current=json.loads(y[0].read_text());keys=['rustc','features','target','profile','rustflags','compile_kind'];assert all(previous[k]==current[k] for k in keys),(old,previous,current)
 refs=subprocess.run(['rg','-l',oh,str(base/'.fingerprint'),'--glob','*.json'],text=True,capture_output=True);assert refs.returncode==0 and {Path(p) for p in refs.stdout.splitlines()}=={x[0]},refs.stdout
 dep=old.with_suffix('.d') if old.suffix=='.exe' else old.with_name(old.name+'.d');files=[old,dep,*a.iterdir()]
 for p in files:assert p.is_file() and not p.is_symlink() and p.stat().st_nlink==1,p
 used=subprocess.run(['fuser',*[str(p) for p in files]],text=True,capture_output=True);assert used.returncode==1,(used.stdout,used.stderr)
 with old.open('rb') as stream:digest=hashlib.file_digest(stream,'sha256').hexdigest()
 plans.append({**row,'old_sha256':digest,'same_compiler_settings':{k:previous[k] for k in keys},'intentional_input_change':'New build script embeds the canonical Windows icon; original locked versions preserved','logical_bytes':sum(p.stat().st_size for p in files),'files':files,'fingerprint':a})
(e/'REPLACED_TEST_PLAN.json').write_text(json.dumps([{k:v for k,v in p.items() if k not in {'files','fingerprint'}} for p in plans],indent=2)+'\n')
before=shutil.disk_usage(r).free
for p in plans:
 for f in p['files']:f.unlink()
 p['fingerprint'].rmdir()
(e/'REPLACED_TEST_CLEANUP.json').write_text(json.dumps({'executables_retired':len(plans),'logical_bytes':sum(p['logical_bytes'] for p in plans),'filesystem_free_delta':shutil.disk_usage(r).free-before,'current_binaries_and_dependencies_preserved':True},indent=2)+'\n')
