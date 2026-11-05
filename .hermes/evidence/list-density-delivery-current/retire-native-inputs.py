"""Retire consumed private transport/bootstrap after safe export and exact teardown."""
from pathlib import Path
import argparse,hashlib,json,subprocess
parser=argparse.ArgumentParser();parser.add_argument('case',choices=['assembly','native-captions','runtime19','runtime19-upgrade-enabled','runtime19-upgrade-disabled']);args=parser.parse_args();r=Path.cwd();e=Path(__file__).resolve().parent;f=e/args.case
read=lambda p:json.loads(p.read_text())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
o=read(f/'OWNER.json');assert o['cleanup']['domain_removed'] and o['cleanup']['fixture_removed'] and len(o['cleanup']['owned_volumes_removed'])==2
current=subprocess.run(['virsh','-c','qemu:///system','domuuid',o['domain']],capture_output=True,text=True);assert current.returncode!=0 or current.stdout.strip()!=o['uuid']
for m in (read(f/'TRANSPORT_CONTENT.json')['members'] if (f/'TRANSPORT_CONTENT.json').exists() else []):
 if not m['private_bootstrap_not_extracted']:
  p=f/'results'/m['name'];assert p.stat().st_size==m['bytes'] and sha(p)==m['sha256']
if 'private_console' in o:assert not Path(o['private_console']['home']).parent.exists()
removed=[]
for name in ['RESULTS.zip','RESULT_EXPORT_PROCESS.json','PREPARATION.json','controller.ps1','GUEST_PREPARATION.json','CONTROLLER_PROCESS.json','DESKTOP_SIGNAL.json','readiness.log','desktop-ready.png','PRIVACY_STEPS.json','dpi-console-current.png']+[p.name for p in f.glob('privacy-accepted-*.png')]:
 p=f/name
 if p.exists():
  assert p.is_file() and not p.is_symlink() and p.stat().st_nlink==1
  assert subprocess.run(['fuser',str(p)],capture_output=True).returncode==1
  removed.append({'path':str(p.relative_to(r)),'bytes':p.stat().st_size,'sha256':sha(p)});p.unlink()
receipt=f/'RETIRED_INPUTS.json';history=read(receipt) if receipt.exists() else {'batches':[]}
if removed:history['batches'].append({'removed':removed,'bytes':sum(v['bytes'] for v in removed)})
history['scope']='Exact terminal case with exported safe results and consumed private bootstrap; other live clone/current packages/base disks untouched';receipt.write_text(json.dumps(history,indent=2)+'\n');print('Retired consumed bytes',sum(v['bytes'] for v in removed))
