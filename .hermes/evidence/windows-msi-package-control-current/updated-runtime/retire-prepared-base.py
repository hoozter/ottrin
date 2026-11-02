"""Retire only the prepared owned base after its final named consumer closes."""
from pathlib import Path
import json,re,subprocess,xml.etree.ElementTree as E
root=Path(__file__).resolve().parent
receipt=Path('.hermes/evidence/windows-msi-updated-client-current/UPDATED_BASE.json')
base=json.loads(receipt.read_text());assessment=json.loads((root/'ASSESSMENT.json').read_text());owner=json.loads((root/'OWNER.json').read_text())
assert assessment['native_runtime']['build']=='26100' and assessment['native_runtime']['msi']=='5.0.26100.9711'
assert assessment['native_service_cleanup_exit']==0 and owner['cleanup']['domain_removed'] and owner['cleanup']['fixture_removed']
assert base['status']=='ready for named pending native consumers' and len(base['pending_consumers'])==1
path=Path(base['path']);volume=base['volume'];assert str(path)=='/var/lib/libvirt/images/ottrin-msi-updated-client.qcow2' and volume==path.name
assert path.stat().st_nlink==1
allocated=path.stat().st_blocks*512

def virsh(*args):return subprocess.check_output(['virsh','-c','qemu:///system',*args],text=True).strip()
domains={};snapshots={}
for name in virsh('list','--all','--name').splitlines():
 xml=virsh('dumpxml',name);assert str(path) not in xml
 domains[name]=virsh('domstate',name);assert domains[name]=='shut off'
 snapshots[name]=virsh('snapshot-list',name,'--name').splitlines()
 for snap in snapshots[name]:assert str(path) not in virsh('snapshot-dumpxml',name,snap)
volumes={}
for line in virsh('vol-list','--pool','default').splitlines()[2:]:
 if not line.strip():continue
 match=re.fullmatch(r'\s*(\S+)\s+(/\S+)\s*',line);assert match,line
 name=match[1];xml=E.fromstring(virsh('vol-dumpxml','--pool','default',name));assert xml.findtext('target/path')==match[2]
 backs=[x.text for x in xml.findall('.//backingStore/path')];assert str(path) not in backs
 volumes[name]={'path':match[2],'backing_paths':backs}
assert volumes[volume]['backing_paths']==['/var/lib/libvirt/images/win11ltsc.qcow2']
protected=['ottrin-copy-cleanup-qa.qcow2','ottrin-index-qa.qcow2','ottrin-qa-20261011.qcow2','win11ltsc.qcow2','ottrin-stock-client.qcow2']
assert all(name in volumes for name in protected)
record={'status':'verified intent before exact owned volume retirement','path':str(path),'allocated_bytes':allocated,'closed_consumer':'Exact held8ede application MSI comparison on26100/.9711; same rollback/retry failure observed, evidence retained','assessment':'ASSESSMENT.json','domains':domains,'snapshots':snapshots,'volumes_before':volumes,'protected_volumes_preserved':protected}
out=root/'PREPARED_BASE_RETIREMENT.json';out.write_text(json.dumps(record,indent=2)+'\n')
virsh('vol-delete','--pool','default',volume);assert not path.exists()
assert all(Path(volumes[name]['path']).exists() for name in protected)
base['status']='retired after all named native consumers completed';base['pending_consumers']=[];base['completed_consumers'].append('Exact retained8ede application MSI on26100/.9711: fresh install58 files pass, genuine refusal damages registration and retry fails1603; same as26300/.9444')
base['retirement_receipt']=str(out);receipt.write_text(json.dumps(base,indent=2)+'\n')
record['status']='owned prepared base removed through libvirt; protected environments preserved';record['path_absent']=True;out.write_text(json.dumps(record,indent=2)+'\n')
print(json.dumps({'removed':str(path),'allocated_bytes_recovered':allocated,'protected_volumes_preserved':protected}))
