"""Retire superseded same-configuration test executables after native replacement checks."""
from pathlib import Path
import hashlib,json,shutil,subprocess
R=Path.cwd();E=Path(__file__).resolve().parent;B=R/'target/x86_64-pc-windows-gnu/debug'
receipt=json.loads((E/'TEST_BINARIES.json').read_text());native=json.loads((E/'NATIVE_ACCEPTANCE.json').read_text());assert native['status'].startswith('pass')
for proc in Path('/proc').iterdir():
 if proc.name.isdigit():
  try:assert (proc/'comm').read_text().strip() not in {'cargo','rustc','rust-analyzer'}
  except (FileNotFoundError,ProcessLookupError):pass
retired=[]
for crate,old in [('ottrin_platform','6753e888d2d2a072'),('ottrin_search','f6138a643ad74388')]:
 current=Path(receipt[crate]['compiler_path']);assert hashlib.file_digest(current.open('rb'),'sha256').hexdigest()==receipt[crate]['compiler_sha256']
 new=current.stem.rsplit('-',1)[1];package=crate.replace('_','-')
 fp=B/'.fingerprint'/f'{package}-{old}';replacement=B/'.fingerprint'/f'{package}-{new}'
 x=list(fp.glob('test-*.json'));y=list(replacement.glob('test-*.json'));assert len(x)==len(y)==1
 a=json.loads(x[0].read_text());z=json.loads(y[0].read_text());keys=['rustc','features','target','profile','rustflags','compile_kind'];assert all(a[k]==z[k] for k in keys)
 refs=subprocess.run(['rg','-l',old,str(B/'.fingerprint'),'--glob','*.json'],capture_output=True,text=True);assert refs.returncode==0 and {Path(p) for p in refs.stdout.splitlines()}=={x[0]}
 binary=B/'deps'/f'{crate}-{old}.exe';files=[binary,binary.with_suffix('.d'),*fp.iterdir()]
 for p in files:assert p.is_file() and not p.is_symlink() and p.stat().st_nlink==1,p
 use=subprocess.run(['fuser',*map(str,files)],capture_output=True,text=True);assert use.returncode==1,(use.stdout,use.stderr)
 row={'removed_executable':str(binary),'current_replacement':str(current),'same_configuration':{k:a[k] for k in keys},'bytes':sum(p.stat().st_size for p in files),'native_replacement_scope':'NATIVE_ACCEPTANCE.json; focused shared-pipe/helper/UAC checks, not a full unrelated platform suite'}
 for p in files:p.unlink()
 fp.rmdir();retired.append(row)
(E/'RETIRED_TESTS.json').write_text(json.dumps({'status':'pass','retired':retired,'current_binaries_dependencies_and_caches_preserved':True},indent=2)+'\n')
print('Retired',len(retired),'superseded test executables and their fingerprints/dep-info')
