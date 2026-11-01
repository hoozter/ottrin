"""Retire exact superseded test outputs after complete replacement execution."""
from pathlib import Path
import hashlib,json,re,shutil,subprocess
r=Path.cwd();e=r/'.hermes/evidence/picker-integration-current'
assert json.loads((e/'RUNTIME_OWNER.json').read_text())['status']=='pass'
assert '432 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out;' in (e/'COMPLETE_NATIVE_FINAL.log').read_text()
def targets(path):
 return {kind: r/artifact for kind,artifact in re.findall(r'Running (.*?) \((target/[^\s)]+)\)',path.read_text())}
old=targets(r/'.hermes/evidence/picker-ui-current/COMPLETE_NATIVE_LOCATION_FIXED.log');new=targets(e/'COMPLETE_NATIVE_FINAL.log');assert old.keys()==new.keys()
pairs=[(a,new[k]) for k,a in old.items() if a!=new[k]]
records=[json.loads(line) for line in (e/'windows-final-build.jsonl').read_text().splitlines() if line.startswith('{')]
assert records[-1]=={'reason':'build-finished','success':True}
windows=[Path(x['executable']) for x in records if x.get('reason')=='compiler-artifact' and x.get('executable') and x['target']['name']=='ottrin_ui'];assert len(windows)==1
pairs.append((r/'target/x86_64-pc-windows-gnu/debug/deps/ottrin_ui-fa30a86b4cca8a3a.exe',windows[0]))
for p in Path('/proc').iterdir():
 if p.name.isdigit():
  try:assert (p/'comm').read_text().strip() not in {'cargo','rustc','rust-analyzer'}
  except (FileNotFoundError,ProcessLookupError):pass
plan=[]
for a,b in pairs:
 assert a.is_file() and b.is_file() and a!=b
 base=a.parent.parent;old_hash=a.stem.rsplit('-',1)[1];new_hash=b.stem.rsplit('-',1)[1]
 crate='ottrin-ui' if a.name.startswith('ottrin_ui-') else 'ottrin-app'
 old_fp=base/'.fingerprint'/f'{crate}-{old_hash}';new_fp=base/'.fingerprint'/f'{crate}-{new_hash}'
 old_json=list(old_fp.glob('test-*.json'));new_json=list(new_fp.glob('test-*.json'));assert len(old_json)==len(new_json)==1
 x=json.loads(old_json[0].read_text());y=json.loads(new_json[0].read_text());keys=['rustc','features','target','profile','rustflags','compile_kind'];assert all(x[k]==y[k] for k in keys)
 refs=subprocess.run(['rg','-l',old_hash,str(base/'.fingerprint'),'--glob','*.json'],text=True,capture_output=True,check=False)
 assert refs.returncode==0 and {Path(p) for p in refs.stdout.splitlines()}=={old_json[0]}
 paths=[a,a.with_suffix('.d') if a.suffix=='.exe' else a.with_name(a.name+'.d'),old_fp]
 for p in [paths[0],paths[1],*old_fp.iterdir()]:assert p.is_file() and not p.is_symlink() and p.stat().st_nlink==1,p
 usage=subprocess.run(['fuser',str(a)],text=True,capture_output=True,check=False);assert usage.returncode==1,(a,usage.stdout,usage.stderr)
 size=sum(p.stat().st_size for p in [paths[0],paths[1],*old_fp.iterdir()])
 plan.append({'old':str(a),'replacement':str(b),'same_compiler_configuration':{k:x[k] for k in keys},'sha256':hashlib.sha256(a.read_bytes()).hexdigest(),'logical_bytes':size,'paths':paths})
(e/'REPLACED_TEST_PLAN.json').write_text(json.dumps([{k:v for k,v in row.items() if k!='paths'} for row in plan],indent=2)+'\n')
before=shutil.disk_usage(r).free
for row in plan:
 for p in row['paths']:
  if p.is_dir():shutil.rmtree(p)
  else:p.unlink()
(e/'REPLACED_TEST_CLEANUP.json').write_text(json.dumps({'retired_artifacts':len(plan),'logical_bytes':sum(row['logical_bytes'] for row in plan),'filesystem_free_delta':shutil.disk_usage(r).free-before,'current_binaries_dependencies_and_test_replacements_preserved':True},indent=2)+'\n')
