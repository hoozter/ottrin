"""Retire only per-project debug artifacts belonging to removed dependency versions."""
from pathlib import Path
from collections import Counter
from contextlib import ExitStack
import fcntl,json,re,shutil,subprocess,tomllib
r=Path.cwd();e=r/'.hermes/evidence/picker-integration-current'
old=tomllib.loads(subprocess.check_output(['git','show','HEAD:Cargo.lock'],text=True))['package'];new=tomllib.load((r/'Cargo.lock').open('rb'))['package']
removed={(x['name'],x['version']) for x in old}-{(x['name'],x['version']) for x in new};assert len(removed)==9
current_names={x['name'] for x in new};roots=[r/'target/debug',r/'target/x86_64-pc-windows-gnu/debug']
assert json.loads((e/'RUNTIME_OWNER.json').read_text())['status']=='pass'
def no_compilers():
 for p in Path('/proc').iterdir():
  if p.name.isdigit():
   try:assert (p/'comm').read_text().strip() not in {'cargo','rustc','rust-analyzer'}
   except (FileNotFoundError,ProcessLookupError):pass
no_compilers();selected=set();proof=[]
for base in roots:
 for name,version in sorted(removed):
  identifier=name.replace('-','_')
  for fp in (base/'.fingerprint').glob(name+'-*'):
   if not fp.is_dir() or fp.is_symlink():continue
   digest=fp.name[len(name)+1:]
   if not re.fullmatch('[0-9a-f]{16}',digest):continue
   dep=base/'deps'/f'{identifier}-{digest}.d'
   exact_version=dep.is_file() and f'/{name}-{version}/'.encode() in dep.read_bytes()
   if name in current_names and not exact_version:continue
   selected.add(fp)
   for pattern in [f'{identifier}-{digest}*',f'lib{identifier}-{digest}*']:
    for p in (base/'deps').glob(pattern):selected.add(p)
   build=base/'build'/fp.name
   if build.is_dir():selected.add(build)
   proof.append({'fingerprint':str(fp),'removed_package':[name,version],
       'basis':'exact dep-info version' if exact_version else 'package entirely absent from current all-target lockfile'})
# Build-script/run fingerprints can have a separate digest; all versions of these
# package names are gone, so their build outputs have no current producer/consumer.
for base in roots:
 for name,version in removed:
  if name not in current_names:
   selected.update(p for p in (base/'build').glob(name+'-*') if p.is_dir())
selected={p for p in selected if not any(parent in selected for parent in p.parents)}
files=[]
for p in selected:
 assert p.is_relative_to(r/'target') and not p.is_symlink()
 children=list(p.rglob('*')) if p.is_dir() else [p]
 for child in children:
  assert not child.is_symlink(),child
  if child.is_file():files.append(child)
links=Counter((p.stat().st_dev,p.stat().st_ino) for p in files)
for p in files:
 info=p.stat();assert links[(info.st_dev,info.st_ino)]==info.st_nlink,('unclassified hard link',p)
plan={'removed_packages':sorted(removed),'version_evidence':proof,'retired_paths':[str(p) for p in sorted(selected)],'logical_bytes':sum(p.stat().st_size for p in files),'consumer_boundary':'Only project native/GNU debug outputs; current manifest/lockfile and completed replacement suites establish obsolescence. Historical cached fingerprints may describe superseded dependency graphs; these are not current compilation consumers. No compiler/analyzer is running; batch file-consumer check and target Cargo locks apply. Existing system process inspection exceptions are documented separately.'}
(e/'REMOVED_DEPENDENCY_PLAN.json').write_text(json.dumps(plan,indent=2)+'\n')
with ExitStack() as locks:
 for base in roots:
  path=base/'.cargo-lock';assert path.is_file() and not path.is_symlink();stream=locks.enter_context(path.open('r+b'));fcntl.flock(stream,fcntl.LOCK_EX|fcntl.LOCK_NB)
 no_compilers()
 if files:
  check=subprocess.run(['fuser',*[str(p) for p in files]],text=True,capture_output=True,check=False)
  assert check.returncode==1,(check.returncode,check.stdout,check.stderr)
 before=shutil.disk_usage(r).free
 for p in selected:
  if p.is_dir():shutil.rmtree(p)
  else:p.unlink()
 after=shutil.disk_usage(r).free
(e/'REMOVED_DEPENDENCY_CLEANUP.json').write_text(json.dumps({'status':'complete','paths_retired':len(selected),'logical_bytes':plan['logical_bytes'],'filesystem_free_delta':after-before,'shared_registry_toolchains_release_outputs_and_current_dependencies_preserved':True},indent=2)+'\n')
