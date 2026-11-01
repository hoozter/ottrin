"""Remove exact superseded test executables after current compiler-matched checks."""
from pathlib import Path
import json,subprocess,hashlib,shutil
r=Path.cwd();e=Path(__file__).resolve().parent
assert json.loads((e/'RUNTIME_OWNER.json').read_text())['status']=='pass'
for token in ['ordinary','elevated']:assert 'test result: ok. 2 passed; 0 failed;' in (e/('short-temp-'+token+'-ui-runtime.stdout')).read_text()
assert 'test result: ok. 23 passed; 0 failed;' in (e/'UI_FINAL_TESTS.log').read_text()
specs=[('target/debug','00f4da14c3c90eed','3ec86bde0f8d579d',''),('target/x86_64-pc-windows-gnu/debug','05937bd1bf220906','5c8836da7b08ab1e','.exe')];plans=[]
for base,oldhash,newhash,suffix in specs:
 base=r/base;old=base/'deps'/('ottrin_ui-'+oldhash+suffix);new=base/'deps'/('ottrin_ui-'+newhash+suffix)
 assert old.exists() and new.exists() and old!=new
 a=base/'.fingerprint'/('ottrin-ui-'+oldhash);b=base/'.fingerprint'/('ottrin-ui-'+newhash)
 x=list(a.glob('test-*.json'));y=list(b.glob('test-*.json'));assert len(x)==len(y)==1
 before=json.loads(x[0].read_text());after=json.loads(y[0].read_text());keys=['rustc','features','target','profile','rustflags','compile_kind'];assert all(before[k]==after[k] for k in keys)
 refs=subprocess.run(['rg','-l',oldhash,str(base/'.fingerprint'),'--glob','*.json'],capture_output=True,text=True);assert refs.returncode==0 and {Path(f) for f in refs.stdout.splitlines()}=={x[0]}
 paths=[old,old.with_suffix('.d'),*a.iterdir()];assert all(p.is_file() and not p.is_symlink() and p.stat().st_nlink==1 for p in paths)
 assert subprocess.run(['fuser',*[str(p) for p in paths]],capture_output=True).returncode==1
 plans.append({'old':str(old),'replacement':str(new),'matching_compiler_settings':{k:before[k] for k in keys},'paths':[str(p) for p in paths],'bytes':sum(p.stat().st_size for p in paths),'old_sha256':hashlib.file_digest(old.open('rb'),'sha256').hexdigest()})
for p in Path('/proc').iterdir():
 try:
  if p.name.isdigit():assert (p/'comm').read_text().strip() not in {'rustc','cargo','rust-analyzer'}
 except (FileNotFoundError,ProcessLookupError):pass
(e/'REPLACED_TEST_PLAN.json').write_text(json.dumps(plans,indent=2)+'\n')
for plan in plans:
 for name in plan['paths']:Path(name).unlink()
 Path(plan['paths'][-1]).parent.rmdir()
(e/'REPLACED_TEST_CLEANUP.json').write_text(json.dumps({'status':'pass','retired':len(plans),'logical_bytes_removed':sum(p['bytes'] for p in plans),'retained':'Current matching test replacements,dependency libraries,current binaries and useful compiler caches','scope':'Focused current chooser and real Win32 ownership execution,not an assertion of repeated full platform acceptance'},indent=2)+'\n')
