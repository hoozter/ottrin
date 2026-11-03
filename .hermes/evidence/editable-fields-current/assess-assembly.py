"""Accept native MakeAppx bytes and retire its consumed source archive."""
from pathlib import Path
import hashlib,json,subprocess,zipfile,xml.etree.ElementTree as E
r=Path.cwd();e=Path(__file__).resolve().parent
read=lambda p:json.loads(p.read_text())
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
a=read(e/'assembly/ASSEMBLY.json');b=read(e/'windows/BUILD_RESULT.json');owner=read(e/'assembly/OWNER.json');p=Path(b['stage'])/Path(a['artifact'].replace('\\','/')).name
assert a['status']=='pass' and a['source_revision']==b['source_revision'] and a['payload_files']==58 and a['rejected_revisions']==[-1,65536];assert sha(p)==a['artifact_sha256'].lower();assert a['provenance']['version']=='0.1.0.9' and not a['provenance']['signed'] and a['provenance']['development']
assert a['source_script_sha256'].lower()==sha(r/'packaging/windows/package-msix.ps1')
manifest=read(e/'PAYLOAD.json')
with zipfile.ZipFile(p) as z:
 for name,digest in manifest.items():assert hashlib.sha256(z.read(name)).hexdigest()==digest,name
 xml=E.fromstring(z.read('AppxManifest.xml'));identity=xml.find('{http://schemas.microsoft.com/appx/manifest/foundation/windows10}Identity');assert identity.attrib['Version']=='0.1.0.9' and identity.attrib['Name']==a['provenance']['package_name'] and identity.attrib['Publisher']==a['provenance']['publisher']
assert owner['cleanup']['domain_removed'] and owner['cleanup']['fixture_removed'] and len(owner['cleanup']['owned_volumes_removed'])==2 and owner['cleanup']['base_still_off']
p.with_name(p.name+'.sha256').write_text(sha(p)+'  '+p.name+'\n');p.with_name(p.name+'.provenance.json').write_text(json.dumps(a['provenance'],indent=2)+'\n')
(e/'ASSEMBLY_ACCEPTANCE.json').write_text(json.dumps({'status':'pass native MSIX9 assembly and58-file equality; runtime/update/login not freshly exercised; prior MSIX8 lifecycle scope retains original hashes','source_revision':a['source_revision'],'artifact':str(p),'artifact_sha256':sha(p),'version':'0.1.0.9','native_payload_files':58,'rejected_revisions':a['rejected_revisions'],'owned_vm_cleanup':owner['cleanup'],'scope':'Actual clean Git source/production MakeAppx assembly, then native unpack and independent host ZIP comparison; no Store/signing/runtime acceptance'},indent=2)+'\n')
s=read(e/'SOURCE_INPUT.json');archive=Path(s['archive'])
if archive.exists():
 assert sha(archive)==s['sha256'] and archive.stat().st_nlink==1 and subprocess.run(['fuser',str(archive)],capture_output=True).returncode==1
 archive.unlink();s.update(status='consumed by native MSIX9 assembly; owned source archive retired',archive_retired=True,cleanup_consumer='assembly/ASSEMBLY.json');(e/'SOURCE_INPUT.json').write_text(json.dumps(s,indent=2)+'\n')
else:
 assert s.get('archive_retired') and s.get('cleanup_consumer')=='assembly/ASSEMBLY.json', 'Missing archive without completed cleanup receipt'
