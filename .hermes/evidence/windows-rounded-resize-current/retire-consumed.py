"""Retire only consumed, receipt-owned inputs; retain current stages for acceptance."""
from pathlib import Path
import hashlib,json,shutil,subprocess
r=Path.cwd();e=Path(__file__).resolve().parent
read=lambda p:json.loads(p.read_text())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
receipt_path=e/'CONSUMED_INPUTS_RETIRED.json'
history=read(receipt_path) if receipt_path.exists() else {'batches':[]}
if 'batches' not in history:history={'batches':[history]}
removed=[]
for label in ['native-pointer','assembly','native-live-radius']:
 folder=e/label;owner=read(folder/'OWNER.json');assert owner['cleanup']['domain_removed'] and owner['cleanup']['fixture_removed'] and len(owner['cleanup']['owned_volumes_removed'])==2
 current=subprocess.run(['virsh','-c','qemu:///system','domuuid',owner['domain']],capture_output=True,text=True)
 assert current.returncode!=0 or current.stdout.strip()!=owner['uuid']
 for vol in owner['cleanup']['owned_volumes_removed']:
  assert subprocess.run(['virsh','-c','qemu:///system','vol-path','--pool','default',vol],capture_output=True).returncode!=0 or label=='native-pointer','A separate fresh native actor owns reused names; old cleanup is UUID scoped'
 if label in ['native-pointer','native-live-radius']:
  members=read(folder/'TRANSPORT_CONTENT.json')['members']
  for member in members:
   if not member['private_bootstrap_not_extracted']:
    p=folder/'results'/member['name'];assert p.stat().st_size==member['bytes'] and sha(p)==member['sha256']
 for name in ['RESULTS.zip','controller.ps1','GUEST_PREPARATION.json','CONTROLLER_PROCESS.json','DESKTOP_SIGNAL.json','readiness.log','desktop-ready.png','PRIVACY_STEPS.json']+[p.name for p in folder.glob('privacy-accepted-*.png')]:
  p=folder/name
  if p.exists():
   assert p.is_file() and not p.is_symlink() and p.stat().st_nlink==1
   assert subprocess.run(['fuser',str(p)],capture_output=True).returncode==1
   removed.append({'path':str(p.relative_to(r)),'bytes':p.stat().st_size,'sha256':sha(p)});p.unlink()
# The production content gate and exact AppImage mouse consumers completed.
owner=read(e/'EXTRACTION_OWNER.json');p=Path(owner['path']);assert p==r/'.hermes/packaging-tmp/.linux-desktop-extract' and not p.is_symlink()
assert all(read(e/name/'TEARDOWN.json')['profile_removed'] for name in ['linux-pointer-dark','linux-pointer-light'])
assert read(e/'PACKAGE_CONTENT_ACCEPTANCE.json')['status']=='pass'
if p.exists():
 mounts=Path('/proc/self/mountinfo').read_text().splitlines();assert not any(str(p) in line for line in mounts)
 files=[f for f in p.rglob('*') if f.is_file() and not f.is_symlink()];assert all(f.stat().st_nlink==1 for f in files)
 # Check every regular file, in bounded argv groups.
 for i in range(0,len(files),100):assert subprocess.run(['fuser',*map(str,files[i:i+100])],capture_output=True).returncode==1
 removed.append({'path':str(p.relative_to(r)),'bytes':sum(f.stat().st_size for f in files),'files':len(files),'reason':'Completed package-content and exact AppImage native GUI consumers; no mounts/open files/hard links'})
 shutil.rmtree(p)
owner.update(pending_consumers=[],cleanup_complete=True,removed=True);(e/'EXTRACTION_OWNER.json').write_text(json.dumps(owner,indent=2)+'\n')
if removed:history['batches'].append({'removed':removed,'bytes':sum(v['bytes'] for v in removed)})
history['scope']='Exact consumed receipt-owned extraction/transports/private bootstrap; package stages retain pending acceptance consumers'
receipt_path.write_text(json.dumps(history,indent=2)+'\n')
print('Retired consumed bytes',sum(v['bytes'] for v in removed))
