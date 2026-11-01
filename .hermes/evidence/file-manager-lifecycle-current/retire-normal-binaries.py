"""Retire only superseded Linux app products with matching compiler settings."""
from pathlib import Path
import json,subprocess,hashlib
r=Path.cwd();e=Path(__file__).resolve().parent;base=r/'target/debug';current=base/'ottrin-app'
assert json.loads((e/'GUI_RESULT.json').read_text())['status']=='pass'
assert hashlib.sha256(current.read_bytes()).hexdigest()==json.loads((e/'GUI_RESULT.json').read_text())['binary_sha256']
for p in Path('/proc').iterdir():
 if p.name.isdigit():
  try:assert (p/'comm').read_text().strip() not in {'cargo','rustc','rust-analyzer'}
  except (FileNotFoundError,ProcessLookupError):pass
inventory=json.loads((e/'NORMAL_BINARY_INVENTORY.json').read_text());same=[row for row in inventory if (base/'deps'/('ottrin_app-'+row['hash'])).samefile(current)];assert len(same)==1;latest=same[0]
keys=['rustc','features','profile','target','path','rustflags','compile_kind'];plans=[];retained=[]
for row in inventory:
 h=row['hash'];exe=base/'deps'/('ottrin_app-'+h)
 if exe.samefile(current):continue
 if not all(row[k]==latest[k] for k in keys):retained.append({'path':str(exe),'reason':'Different compiler configuration'});continue
 fp=base/'.fingerprint'/('ottrin-app-'+h);record=fp/'bin-ottrin-app.json';assert all(json.loads(record.read_text())[k]==row[k] for k in keys)
 refs=subprocess.run(['rg','-l',h,str(base/'.fingerprint'),'--glob','*.json'],text=True,capture_output=True);assert refs.returncode==0 and {Path(p) for p in refs.stdout.splitlines()}=={record},refs.stdout
 files=[exe,exe.with_name(exe.name+'.d'),*fp.iterdir()]
 if any(not p.is_file() or p.is_symlink() or p.stat().st_nlink!=1 for p in files):retained.append({'path':str(exe),'reason':'Non-disposable type or hard-link consumer'});continue
 used=subprocess.run(['fuser',*[str(p) for p in files]],text=True,capture_output=True)
 if used.returncode==0:retained.append({'path':str(exe),'reason':'Active consumer','detail':used.stdout+used.stderr});continue
 assert used.returncode==1
 with exe.open('rb') as stream:digest=hashlib.file_digest(stream,'sha256').hexdigest()
 plans.append({'path':str(exe),'sha256':digest,'matching_compiler_settings':{k:row[k] for k in keys},'bytes':sum(p.stat().st_size for p in files),'files':files,'fingerprint':fp})
(e/'NORMAL_BINARY_CLEANUP_PLAN.json').write_text(json.dumps({'replacement':str(current),'replacement_sha256':hashlib.sha256(current.read_bytes()).hexdigest(),'remove':[{k:v for k,v in row.items() if k not in {'files','fingerprint'}} for row in plans],'retain':retained},indent=2)+'\n')
for row in plans:
 for p in row['files']:p.unlink()
 row['fingerprint'].rmdir()
result={'retired_executables':len(plans),'logical_bytes':sum(p['bytes'] for p in plans),'retained':retained,'current_source_binary_and_shared_dependencies_preserved':True};(e/'NORMAL_BINARY_CLEANUP.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))
