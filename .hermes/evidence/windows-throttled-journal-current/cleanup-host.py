from pathlib import Path
import json,subprocess,sys,time,xml.etree.ElementTree as E
p=Path(__file__).resolve().parent;root=Path.cwd();sys.path.insert(0,str(root/'.hermes/evidence/windows-copy-cleanup-current'));import transport as n
o=json.loads((p/'OWNERSHIP.json').read_text());domain=o['domain'];source='/var/lib/libvirt/images/'+o['secondary_volume']
def virsh(*args):return subprocess.check_output(['virsh',*args],text=True).strip()
def volume_names():
 rows=[line.split() for line in virsh('vol-list','default').splitlines()[2:] if line.strip()]
 assert all(len(row)==2 and row[1].startswith('/') for row in rows),'Unexpected pool listing layout'
 return [row[0] for row in rows]
def retire_volume():
 assert virsh('domstate',domain)=='shut off'
 assert json.loads((p/'TEARDOWN_GUEST.json').read_text(encoding='utf-8-sig'))['status']=='pass'
 assert virsh('dumpxml',domain,'--inactive')==(p/'DOMAIN_BEFORE.xml').read_text().strip(),'Persistent guest definition changed'
 for name in virsh('list','--all','--name').splitlines():
  assert source not in virsh('dumpxml',name,'--inactive')
  for snapshot in virsh('snapshot-list',name,'--name').splitlines():assert source not in virsh('snapshot-dumpxml',name,snapshot)
 volume=E.fromstring(virsh('vol-dumpxml','--pool','default',o['secondary_volume']))
 assert volume.find('./target/path').text==source
 assert volume.findtext('./backingStore/path') in [None,'']
 for name in volume_names():
  if name==o['secondary_volume']:continue
  xml=E.fromstring(virsh('vol-dumpxml','--pool','default',name))
  assert all(node.text!=source for node in xml.findall('.//backingStore/path'))
 virsh('vol-delete','--pool','default',o['secondary_volume'])
 assert o['secondary_volume'] not in volume_names()
 receipt={'status':'pass','guest':'TEARDOWN_GUEST.json','domain_state':'shut off','persistent_domain_definition_unchanged':True,'task_disk_detached_and_deleted_via_libvirt':True,'domain_and_snapshot_references_clear':True,'retained_base_and_other_guests_preserved':True}
 (p/'TEARDOWN.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(receipt),flush=True)
if len(sys.argv)>1:
 assert sys.argv[1:] == ['--retire-detached-volume']
 retire_volume();raise SystemExit(0)
disks=E.fromstring(virsh('dumpxml',domain)).findall('./devices/disk')
matching=[d for d in disks if d.find('target').get('dev')==o['target']]
assert len(matching)==1 and matching[0].find('source').get('file')==source
assert matching[0].find('serial').text==o['secondary_serial']
tune=dict(line.split(':',1) for line in virsh('blkdeviotune',domain,o['target'],'--live').splitlines() if ':' in line)
assert int(tune['write_bytes_sec'].strip())==0
result=json.loads((p/'same-plan-stop/RESULTS.json').read_text(encoding='utf-8-sig'));assert result['status']=='pass'
script=(p/'teardown.ps1').read_text().replace('EXPECTED_HASH',result['test_binary_hash'])
pid=n.start(script);(p/'teardown.pid').write_text(str(pid));n.wait(pid,45)
n.download('IO_TEARDOWN.json',p/'TEARDOWN_GUEST.json')
virsh('detach-disk',domain,o['target'],'--live')
deadline=time.monotonic()+30
while any(d.find('target').get('dev')==o['target'] for d in E.fromstring(virsh('dumpxml',domain)).findall('./devices/disk')):
 assert time.monotonic()<deadline,'Live disk detach still pending';time.sleep(.5)
virsh('shutdown',domain,'--mode','agent')
deadline=time.monotonic()+90
while virsh('domstate',domain)!='shut off':
 assert time.monotonic()<deadline,'Guest shutdown still pending; inspect same domain';time.sleep(1)
retire_volume()
