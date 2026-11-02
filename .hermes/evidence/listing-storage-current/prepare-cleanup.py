"""Validate references/configuration/links and prepare only exact retired test outputs."""
from pathlib import Path
import hashlib,importlib.util,json,os,stat,subprocess
r=Path.cwd();e=Path(__file__).resolve().parent;a=json.loads((e/'TEST_OUTPUT_CANDIDATES.json').read_text());verified=json.loads((e/'REPLACEMENT_VERIFICATION.json').read_text());assert verified['status'].startswith('pass')
sp=importlib.util.spec_from_file_location('cleanup','/home/campbell/.codex/skills/computer-cleanup/scripts/cleanup.py');cleanup=importlib.util.module_from_spec(sp);sp.loader.exec_module(cleanup)
paths=[Path(p) for c in a['candidates'] for p in c['obsolete']['paths']];assert all(p.exists() for p in paths)
for c in a['candidates']:
 old=c['obsolete'];new=c['replacement'];x=json.loads(Path(old['fingerprint']).read_text());y=json.loads(Path(new['fingerprint']).read_text())
 assert all(x[k]==y[k] for k in c['group'] if k in x)
 assert Path(new['binary']).stat().st_mtime_ns>Path(old['binary']).stat().st_mtime_ns
 assert all(p.is_file() and not p.is_symlink() and p.stat().st_nlink==1 for p in [Path(old['binary']),Path(old['paths'][1]),*Path(old['paths'][2]).iterdir()])
for proc in Path('/proc').iterdir():
 if not proc.name.isdigit():continue
 try:
  comm=(proc/'comm').read_text().strip()
  if comm in ['rustc','cargo','rust-analyzer']:
   cwd=Path(os.readlink(proc/'cwd'));assert not cwd.is_relative_to(r),('Active workspace compiler consumer',int(proc.name),comm)
 except (FileNotFoundError,ProcessLookupError):pass
inventory=json.loads(Path('/home/campbell/.local/state/computer-cleanup/inventory.json').read_text());incoming=[]
# Resolve the inventory's symlink graph in memory. Do not traverse network/FUSE
# targets or repeat filesystem resolution across89k unrelated installed links.
links={os.path.normpath(link['path']):link['target'] for link in inventory['symlinks']}
def resolve_inventory(raw,depth=0):
 assert depth<100,'Symlink cycle while resolving an incoming candidate'
 components=list(Path(raw).parts);stack=[]
 while components:
  part=components.pop(0)
  if part=='/':stack=['/'];continue
  if part=='.':continue
  if part=='..':
   if len(stack)>1:stack.pop()
   continue
  stack.append(part);current=str(Path(*stack))
  if current in links:
   target=Path(links[current]);replacement=target if target.is_absolute() else Path(current).parent/target
   return resolve_inventory(str(replacement.joinpath(*components)),depth+1)
 return Path(*stack)
for link in inventory['symlinks']:
 target=Path(link['target']);target=target if target.is_absolute() else Path(link['path']).parent/target
 # Every incoming alias must terminate at one of these exact local paths.
 try:target=resolve_inventory(str(target))
 except AssertionError:continue  # A cyclic unrelated installed link cannot reach a test output.
 if not target.is_relative_to(r/'target'):continue
 if any(target==q or target.is_relative_to(q) for q in paths):incoming.append(link)
assert not incoming,incoming
references=(e/'REFERENCE_FILES.txt').read_text().splitlines();scripts=[p for p in references if p.endswith(('.py','.sh','.ps1','.md','.yml','.toml'))];assert set(scripts)=={'./.hermes/evidence/services-refresh-current/retire-ui-test.py','./.hermes/evidence/windows-folder-integration-current/retire-platform-test.py'}
for folder,old,receipt in [('services-refresh-current','target/x86_64-pc-windows-gnu/debug/deps/ottrin_ui-9f89d5d24748fe9b.exe','REPLACED_UI_TEST_CLEANUP.json'),('windows-folder-integration-current','target/x86_64-pc-windows-gnu/debug/deps/ottrin_platform-c40aea5fd597ebbb.exe','REPLACED_PLATFORM_TEST_CLEANUP.json')]:
 assert not (r/old).exists();assert (r/'.hermes/evidence'/folder/receipt).is_file()
reviews=json.loads((r/'.hermes/evidence/listing-metadata-current/PROCESS_REVIEWS.json').read_text())['process_reviews']
for review in reviews:
 current=cleanup.process_identity(review['pid']);assert all(review[k]==v for k,v in current.items());process=Path('/proc')/str(review['pid']);status=dict(line.split(':',1) for line in (process/'status').read_text().splitlines() if ':' in line);assert status['PPid'].strip()=='1202'
 binary=Path(review['declared_system_executable']);assert hashlib.sha256(binary.read_bytes()).hexdigest()==review['system_executable_sha256']
 for p in [binary,*binary.parents]:assert p.stat().st_uid==0 and not p.stat().st_mode&0o022
 md5=Path('/var/lib/dpkg/info/'+review['package']+'.md5sums').read_text().splitlines();line=next(line for line in md5 if line.split(maxsplit=1)[1]==str(binary).lstrip('/'));assert hashlib.md5(binary.read_bytes()).hexdigest()==line.split()[0]
 review['reason']='Existing root-owned desktop PAM/authentication process, same PID/start/command identity, PPid1202 and installed-package bytes revalidated. Exact standalone obsolete test files are unrelated to this service; inaccessible mappings are an explicitly reviewed gap, not claimed read. Current replacements/configurations, historical refs, incoming symlinks and normal accessible consumers checked independently.'
entries=[{'path':str(p),'kind':'build-cache','reason':'Superseded standalone Ottrin test output with a strictly newer matching compiler/configuration replacement retained','evidence':'TEST_OUTPUT_CANDIDATES.json and REPLACEMENT_VERIFICATION.json; exact compiler/target/profile/source/features/flags match; complete native harness/PE validation; no current caller, incoming symlink or hard link. Historical logs retained. Generic helper rechecks live processes and unchanged files.'} for p in paths]
manifest={'entries':entries,'process_reviews':reviews};destination=Path('/home/campbell/.local/state/computer-cleanup/removals.json');destination.write_text(json.dumps(manifest,indent=2)+'\n');destination.chmod(0o600)
(e/'REFERENCE_REVIEW.json').write_text(json.dumps({'status':'pass within exact obsolete standalone-test scope','candidate_executables':len(a['candidates']),'exact_removal_paths':len(paths),'incoming_symlinks':incoming,'matching_reference_files':references,'offline_scripts':'Both are completed one-shot cleanup producers; their old input is already absent and completion receipt exists. Remaining references are immutable historical logs/acceptance/build records, not active callers. Current package/runtime consumers use different paths.','process_reviews':reviews,'unreadable_global_inventory_paths':len(inventory['errors']),'scope_limit':'Whole-machine protected inventory not complete; no deletion outside exact user-owned Ottrin test outputs.'},indent=2)+'\n')
subprocess.run(['python3','/home/campbell/.codex/skills/computer-cleanup/scripts/cleanup.py','prepare',str(destination)],check=True)
