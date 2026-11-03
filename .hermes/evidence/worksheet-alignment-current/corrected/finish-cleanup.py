"""Retire consumed QA transport only; preserve raw receipts and stable builds."""
from pathlib import Path
import hashlib,json,subprocess,shutil
r=Path.cwd();e=Path(__file__).resolve().parent
read=lambda p:json.loads(p.read_text())
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
removed=[];cases=['upgrade-enabled','upgrade-enabled-native-focus','upgrade-disabled-native-focus','upgrade-disabled-console']
for case in cases:
 p=e/case
 if not (p/'results').exists():subprocess.run(['python3',str(e/'collect-runtime.py'),'--case',str(p)],check=True)
 members=read(p/'RESULT_MEMBERS.json');archive=p/'RESULTS.zip';assert sha(archive)==members['archive_sha256']
 for name,digest in members['members'].items():assert sha(p/'results'/name)==digest
 assert archive.stat().st_nlink==1 and subprocess.run(['fuser',str(archive)],capture_output=True).returncode==1;removed.append({'path':str(archive),'bytes':archive.stat().st_size,'purpose':'Duplicate transport retired after exact raw member verification'});archive.unlink()
 for name in ['current-desktop.png','console-watch.png']:
  transient=p/name
  if transient.exists():removed.append({'path':str(transient),'bytes':transient.stat().st_size,'purpose':'Completed transient console observation; native product/desktop readiness captures retained'});transient.unlink()
 if (p/'readiness-review.png').exists():
  assert sha(p/'readiness-review.png')==sha(p/'desktop-ready.png');removed.append({'path':str(p/'readiness-review.png'),'bytes':(p/'readiness-review.png').stat().st_size,'purpose':'Byte-identical duplicate of retained desktop-ready.png'});(p/'readiness-review.png').unlink()
# Every owned VM, volume, private desktop/profile must be gone before completion.
fixtures=[]
for case in ['windows-native','assembly',*cases]:
 p=e/case;o=read(p/'OWNER.json');c=o['cleanup'];assert c['domain_removed'] and c['fixture_removed'] and c['base_still_off'];assert o['domain'] not in subprocess.check_output(['virsh','-c','qemu:///system','list','--all','--name'],text=True).splitlines()
 for volume in c['owned_volumes_removed']:assert subprocess.run(['virsh','-c','qemu:///system','vol-info','--pool','default',volume],capture_output=True).returncode!=0
 viewer=p/'viewer/TEARDOWN.json'
 if viewer.exists():v=read(viewer);assert v['profile_removed'] and not Path(v['profile']).exists()
 fixtures.append({'case':case,'cleanup':c})
for stage in ['.linux-native-desktop-stage','.windows-picker-stage','.windows-candidate-retired','.linux-desktop-extract','.windows-msix-source']:assert not (r/'.hermes/packaging-tmp'/stage).exists()
assert not (e/'source.zip').exists() and not (e/'previous-input.msix').exists()
for p in e.rglob('__pycache__'):
 assert p.is_dir();shutil.rmtree(p)
(e/'CLEANUP.json').write_text(json.dumps({'status':'complete for worksheet delivery task; no owned running test, VM, profile, staging or archive remains','source_revision':read(e/'WINDOWS_DELIVERY.json')['source_revision'],'owned_native_fixtures':fixtures,'owned_volume_count_removed':sum(len(v['cleanup']['owned_volumes_removed']) for v in fixtures),'retired_transports':removed,'incremental_cleanup':read(e/'INCREMENTAL_CLEANUP.json'),'remaining_task_storage':[{'path':str(e),'purpose':'Compact acceptance receipts, actual rendered captures and reproducible QA source; no pending temporary consumer'},{'path':str(r/'dist/packages'),'purpose':'Current four-format Linux deliverables'},{'path':str(r/'dist/windows-candidate'),'purpose':'Current ZIP/NSIS/MSIX6 deliverables and complete58-file current payload'}],'preserved':'Untracked AGENTS.md, user profiles/data/processes, protected Windows bases/retained environments, current shared compiler caches, separately held failing MSI diagnostic'},indent=2)+'\n')
print('All six native fixtures/twelve volumes gone; consumed transport and duplicate captures retired.')
