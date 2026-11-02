"""Remove only exact superseded standalone binaries with verified current peers."""
from pathlib import Path
import hashlib,importlib.util,json,os,re,shutil,sys
root=Path.cwd();e=Path(__file__).resolve().parent
base=json.loads((e/'BUILD_BASELINE.json').read_text())['files'];reviews=json.loads((e/'PROCESS_REVIEWS.json').read_text())['process_reviews']
sys.path.insert(0,'/home/campbell/.codex/skills/computer-cleanup/scripts');import cleanup
current=[]
for line in (e/'tests-final.stderr').read_text().splitlines():
 m=re.search(r'^     Running .* \((target/debug/deps/[^)]+)\)$',line)
 if m:current.append(root/m[1])
current.append(root/'target/debug/ottrin-app')
records=[];all_paths=[]
for new in current:
 assert new.is_file() and new.read_bytes()[:4]==b'\x7fELF'
 if new.name=='ottrin-app':
  aliases=[q for q in (root/'target/debug/deps').iterdir() if q.is_file() and q.stat().st_ino==new.stat().st_ino]
  assert len(aliases)==1 and new.stat().st_nlink==2
  suffix=aliases[0].name.rsplit('-',1)[1];fp=root/'target/debug/.fingerprint'/('ottrin-app-'+suffix)/'bin-ottrin-app.json';reference=json.loads(fp.read_text());olds=[]
  verified_hashes={json.loads(p.read_text())['binary_sha256'] for p in e.glob('*/NATIVE_RESULT.json')}
  for prior in (root/'target/debug/.fingerprint').glob('ottrin-app-*/bin-ottrin-app.json'):
   old=root/'target/debug/deps'/('ottrin_app-'+prior.parent.name.rsplit('-',1)[1])
   if not old.is_file() or old.stat().st_ino==new.stat().st_ino:continue
   metadata=json.loads(prior.read_text());keys=['rustc','features','declared_features','target','profile','path','rustflags','config','compile_kind']
   if not all(reference.get(k)==metadata.get(k) for k in keys):continue
   if str(old.relative_to(root)) in base or hashlib.file_digest(old.open('rb'),'sha256').hexdigest() in verified_hashes:olds.append(old)

 else:
  stem,hash_value=new.name.rsplit('-',1);matches=list((root/'target/debug/.fingerprint').glob('*-'+hash_value+'/*.json'));assert len(matches)==1,(new,matches)
  fp=matches[0];reference=json.loads(fp.read_text());olds=[]
  for q in (root/'target/debug/deps').glob(stem+'-*'):
   if str(q.relative_to(root)) not in base or q==new or q.suffix or not q.is_file() or not q.stat().st_mode&0o111:continue
   prior=fp.parent.with_name(fp.parent.name.rsplit('-',1)[0]+'-'+q.name.rsplit('-',1)[1])/fp.name
   if not prior.exists():continue
   old=json.loads(prior.read_text());keys=['rustc','features','declared_features','target','profile','path','rustflags','config','compile_kind']
   if all(reference.get(k)==old.get(k) for k in keys):olds.append(q)
 for old in olds:
  assert old.read_bytes()[:4]==b'\x7fELF'
  suffix=old.name.rsplit('-',1)[1];package='ottrin-ui' if old.name.startswith('ottrin_ui-') else 'ottrin-app'
  fpdir=root/'target/debug/.fingerprint'/(package+'-'+suffix);dep=old.with_suffix('.d');assert fpdir.is_dir() and dep.is_file()
  assert old.stat().st_nlink==1
  paths=[old,dep,fpdir]
  all_paths.extend(paths);records.append({'old':str(old),'bytes':old.stat().st_size,'replacement':str(new),'replacement_sha256':hashlib.file_digest(new.open('rb'),'sha256').hexdigest(),'paths':[str(p) for p in paths],'basis':'Same compiler/profile/rustflags/target identity for tests; authoritative AccessKit production configuration replaces former application binary; current peers built and test suites passed'})
cleanup.check_consumers(all_paths,reviews)
for item in records:
 for n in item['paths']:
  p=Path(n)
  if p.is_dir():shutil.rmtree(p)
  else:p.unlink()
(e/'STANDALONE_CLEANUP.json').write_text(json.dumps({'status':'verified superseded standalone binaries, exact dep-info/fingerprints removed','items':records,'recovered_binary_bytes':sum(x['bytes'] for x in records),'current_executables_and_shared_libraries_preserved':True},indent=2)+'\n')
print(json.dumps({'removed_binaries':len(records),'recovered_binary_bytes':sum(x['bytes'] for x in records)}))
