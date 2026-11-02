"""Verify official Windows hash and extract only required Fedora bootstrap payloads."""
from pathlib import Path
import hashlib,json,shutil,subprocess
r=Path.cwd();e=Path(__file__).resolve().parent;cache=r/'.hermes/tools/windows-stock-client';pins=json.loads((e/'INPUT_PINS.json').read_text());w=cache/pins['windows']['filename'];v=cache/pins['virtio']['filename'];out=cache/'bootstrap';assert not out.exists()
def sha(p):
 with p.open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest()
for p,key,owner_name in [(w,'windows','DOWNLOAD_OWNER.json'),(v,'virtio','VIRTIO_DOWNLOAD_OWNER.json')]:
 owner=json.loads((e/owner_name).read_text());assert owner['status'].startswith('downloaded') and p.stat().st_size==pins[key]['bytes'];assert sha(p)==owner['sha256']
 if key=='windows':assert owner['sha256']==pins[key]['sha256']
files=['guest-agent/qemu-ga-x86_64.msi','vioserial/w11/amd64/vioser.cat','vioserial/w11/amd64/vioser.inf','vioserial/w11/amd64/vioser.sys'];owner={'status':'extracting only required bootstrap','path':str(out),'source_iso':str(v),'files':files,'pending_consumer':'New independent Windows installer bootstrap; driver catalog and agent Authenticode inspected by normal Windows before install'};(e/'BOOTSTRAP_OWNER.json').write_text(json.dumps(owner,indent=2)+'\n')
try:
 out.mkdir();subprocess.run(['7z','x','-y','-o'+str(out),str(v),*files],check=True,stdout=subprocess.DEVNULL)
 assert sorted(str(p.relative_to(out)) for p in out.rglob('*') if p.is_file())==sorted(files)
 owner.update(status='extracted; native trust/bootstrap acceptance pending',payloads=[{'path':p,'bytes':(out/p).stat().st_size,'sha256':sha(out/p)} for p in files],source_iso_sha256=sha(v))
except BaseException as error:
 shutil.rmtree(out);owner.update(status='failed extraction; owned output removed',error=repr(error));raise
finally:(e/'BOOTSTRAP_OWNER.json').write_text(json.dumps(owner,indent=2)+'\n')
assert v.stat().st_nlink==1;assert subprocess.run(['fuser',str(v)],capture_output=True).returncode==1;size=v.stat().st_size;v.unlink();owner.update(source_iso_removed_bytes=size,source_iso_cleanup='Sole selected-payload extraction consumer fulfilled; no driver ISO VM attachment required');(e/'BOOTSTRAP_OWNER.json').write_text(json.dumps(owner,indent=2)+'\n')
(e/'WINDOWS_SOURCE_VERIFICATION.json').write_text(json.dumps({'status':'pass: exact Microsoft published ISO SHA256','source':pins['windows']['provenance'],'sha256':sha(w),'bytes':w.stat().st_size,'path':str(w),'scope':'Input authenticity/identity, not installed Windows, MSI or package acceptance'},indent=2)+'\n')
print('Microsoft ISO verified; required bootstrap payloads extracted; full virtio ISO removed')
