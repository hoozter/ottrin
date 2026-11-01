"""Retire the directly superseded GNU platform test executable, retaining current caches."""
from pathlib import Path
import json,subprocess,hashlib,shutil
r=Path.cwd();e=r/'.hermes/evidence/windows-folder-integration-current';b=r/'target/x86_64-pc-windows-gnu/debug'
assert '9 passed; 0 failed;' in (e/'native-final/short-temp-ordinary-platform-runtime.stdout').read_text()
assert '9 passed; 0 failed;' in (e/'native-final/short-temp-elevated-platform-runtime.stdout').read_text()
a=b/'deps/ottrin_platform-c40aea5fd597ebbb.exe';z=b/'deps/ottrin_platform-5d4b21ab5d11dd66.exe'
assert hashlib.sha256(z.read_bytes()).hexdigest()==json.loads((e/'native-final/RUNTIME_BINARIES.json').read_text())['fixture-platform.exe']['raw_sha256']
for p in Path('/proc').iterdir():
 if p.name.isdigit():
  try:assert (p/'comm').read_text().strip() not in {'cargo','rustc','rust-analyzer'}
  except (FileNotFoundError,ProcessLookupError):pass
fp=b/'.fingerprint/ottrin-platform-c40aea5fd597ebbb';new=b/'.fingerprint/ottrin-platform-5d4b21ab5d11dd66'
x=list(fp.glob('test-*.json'));y=list(new.glob('test-*.json'));assert len(x)==len(y)==1
old=json.loads(x[0].read_text());current=json.loads(y[0].read_text());keys=['rustc','features','target','profile','rustflags','compile_kind'];assert all(old[k]==current[k] for k in keys)
refs=subprocess.run(['rg','-l','c40aea5fd597ebbb',str(b/'.fingerprint'),'--glob','*.json'],text=True,capture_output=True);assert refs.returncode==0 and {Path(p) for p in refs.stdout.splitlines()}=={x[0]},refs.stdout
files=[a,a.with_suffix('.d'),*fp.iterdir()]
for p in files:assert p.is_file() and not p.is_symlink() and p.stat().st_nlink==1,p
use=subprocess.run(['fuser',*[str(p) for p in files]],text=True,capture_output=True);assert use.returncode==1,(use.stdout,use.stderr)
receipt={'retired_executable':str(a),'current_replacement':str(z),'same_compiler_configuration':{k:old[k] for k in keys},'logical_bytes':sum(p.stat().st_size for p in files),'replacement_execution_scope':'Nine native folder/parser/registry tests under ordinary and elevated Windows tokens; full unrelated platform suite not rerun here','current_dependencies_and_binaries_preserved':True}
before=shutil.disk_usage(r).free
for p in files:p.unlink()
fp.rmdir();receipt['filesystem_free_delta']=shutil.disk_usage(r).free-before
(e/'REPLACED_PLATFORM_TEST_CLEANUP.json').write_text(json.dumps(receipt,indent=2)+'\n')
