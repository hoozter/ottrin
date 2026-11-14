"""Accept native MakeAppx bytes and retire its consumed source archive."""
from pathlib import Path
import hashlib,json,subprocess,zipfile,xml.etree.ElementTree as E
r=Path.cwd();e=Path(__file__).resolve().parent
read=lambda p:json.loads(p.read_text())
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
a=read(e/'assembly40/ASSEMBLY.json');b=read(e/'HOST_PAYLOAD.json');owner=read(e/'assembly40/OWNER.json');p=Path(b['stage'])/Path(a['artifact'].replace('\\','/')).name
assert a['status']=='pass' and a['source_revision']==b['source_revision'] and a['payload_files']==59 and a['rejected_revisions']==[-1,65536];assert sha(p)==a['artifact_sha256'].lower();assert a['provenance']['version']=='0.1.0.40' and not a['provenance']['signed'] and a['provenance']['development']
assert a['source_script_sha256'].lower()==sha(r/'packaging/windows/package-msix.ps1')
manifest=read(e/'HOST_PAYLOAD.json')['payload_hashes']
daemon=a['daemon_runtime'];assert daemon['status'].startswith('pass')
assert daemon['binary_sha256']==manifest['bin/ottrin-indexd.exe']
assert daemon['initial_ready']['watcher_ready'] and daemon['restart_ready']['watcher_ready']
assert daemon['initial_ready']['indexed_items']==daemon['restart_ready']['indexed_items']==1000
assert daemon['watcher_mutations']==['create','rename','delete'] and daemon['protocol_shutdowns']==2
assert daemon['idle']['criterion_below_one_percent'] and daemon['idle']['cpu_one_core_percent']<1 and daemon['idle']['seconds']>=30
assert daemon['source_and_preferences_preserved'] and daemon['private_state_removed']
with zipfile.ZipFile(p) as z:
 for name,digest in manifest.items():assert hashlib.sha256(z.read(name)).hexdigest()==digest,name
 xml=E.fromstring(z.read('AppxManifest.xml'));identity=xml.find('{http://schemas.microsoft.com/appx/manifest/foundation/windows10}Identity');assert identity.attrib['Version']=='0.1.0.40' and identity.attrib['Name']==a['provenance']['package_name'] and identity.attrib['Publisher']==a['provenance']['publisher']
assert owner['cleanup']['domain_removed'] and owner['cleanup']['fixture_removed'] and len(owner['cleanup']['owned_volumes_removed'])==2 and owner['cleanup']['base_still_off']
(e/'WINDOWS_DAEMON_RUNTIME.json').write_text(json.dumps(daemon|{'source_revision':a['source_revision'],'owned_vm_cleanup':owner['cleanup']},indent=2)+'\n')
p.with_name(p.name+'.sha256').write_text(sha(p)+'  '+p.name+'\n');p.with_name(p.name+'.provenance.json').write_text(json.dumps(a['provenance'],indent=2)+'\n')
(e/'ASSEMBLY_ACCEPTANCE.json').write_text(json.dumps({'status':'pass native MSIX 40 assembly and 59-file equality; runtime/update/login have separate acceptance receipts','source_revision':a['source_revision'],'artifact':str(p),'artifact_sha256':sha(p),'version':'0.1.0.40','native_payload_files':59,'rejected_revisions':a['rejected_revisions'],'owned_vm_cleanup':owner['cleanup'],'scope':'Actual clean Git source/production MakeAppx assembly, then native unpack and independent host ZIP comparison; no Store/signing/runtime acceptance'},indent=2)+'\n')
s=read(e/'SOURCE_INPUT.json');archive=Path(s['archive'])
assert not archive.exists() and s['temporary_clone_removed']
s.update(status='consumed by native MSIX 40 assembly; owned source archive retired',archive_retired=True,cleanup_consumer='assembly40/ASSEMBLY.json')
(e/'SOURCE_INPUT.json').write_text(json.dumps(s,indent=2)+'\n')
