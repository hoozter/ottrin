from pathlib import Path
import hashlib,json,os,shutil
repo=Path.cwd();p=Path(__file__).resolve().parent;candidate=repo/'.hermes/packaging-tmp/eps-refresh';stable=repo/'dist/packages';current=repo/'.hermes/packaging-tmp/windows-current'
def sha(q):return hashlib.sha256(q.read_bytes()).hexdigest()
linux=json.loads((p/'LINUX_PACKAGES_ACCEPTANCE.json').read_text());win=json.loads((candidate/'windows/CANDIDATE_ACCEPTANCE.json').read_text());oldlinux=json.loads((stable/'LINUX_PACKAGES_ACCEPTANCE.json').read_text());oldwin=json.loads((current/'CANDIDATE_ACCEPTANCE.json').read_text())
assert json.loads((p/'FINAL_CLEANUP.json').read_text())['status']=='pass'
assert linux['source_revision']==win['source_revision']=='e9e678372c23e2476e98145e4585a993ae5f23e9'
for name,digest in linux['artifacts'].items():assert sha(candidate/name)==digest and sha(stable/name)==oldlinux['artifacts'][name]
for name,digest in win['artifacts'].items():assert sha(candidate/'windows'/name)==digest and sha(current/name)==oldwin['artifacts'][name]
# Never remove a cohort while a live consumer still has its files open/mapped.
consumers=[]
for proc in Path('/proc').iterdir():
 try:
  if not proc.name.isdigit() or proc.stat().st_uid!=os.getuid():continue
  for fd in (proc/'fd').iterdir():
   try:target=os.readlink(fd)
   except OSError:continue
   if target.startswith(str(current)+'/'):consumers.append((proc.name,'open file'))
  if str(current)+'/' in (proc/'maps').read_text():consumers.append((proc.name,'mapped file'))
 except OSError:continue
assert not consumers,consumers
# Per-artifact replacement is atomic. The old AppImage's existing open inode
# remains valid for any running operator daemon; no profile/service is changed.
for name in linux['artifacts']:
 for suffix in ['', '.sha256', '.provenance.json']:
  source=candidate/(name+suffix);assert source.is_file(),source;os.replace(source,stable/source.name)
 receipt=p/'LINUX_PACKAGES_ACCEPTANCE.json'
# Publish the cohort receipt last, after verifying every replaced artifact.
for name,digest in linux['artifacts'].items():assert sha(stable/name)==digest
pending=stable/'.LINUX_PACKAGES_ACCEPTANCE.pending';shutil.copyfile(receipt,pending);os.replace(pending,stable/'LINUX_PACKAGES_ACCEPTANCE.json')
backup=current.with_name('windows-superseded-eps');assert not backup.exists();os.replace(current,backup)
try:os.replace(candidate/'windows',current)
except BaseException:os.replace(backup,current);raise
for name,digest in win['artifacts'].items():assert sha(current/name)==digest
shutil.rmtree(backup);candidate.rmdir()
(p/'PROMOTION.json').write_text(json.dumps({'status':'pass','source_revision':linux['source_revision'],'linux_stable':str(stable),'windows_internal_stable':str(current),'superseded_generated_packages':'Replaced; previous internal Windows cohort removed after consumer check','operator_service_and_profiles':'Not mutated','candidate_directory_removed':not candidate.exists(),'artifacts':linux['artifacts']|win['artifacts']},indent=2)+'\n')
print('Promoted four Linux artifacts and one internal three-format Windows cohort; superseded staging removed')
