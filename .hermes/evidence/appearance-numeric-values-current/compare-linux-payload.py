"""Compare every old/new AppImage file before carrying unchanged acceptance scopes."""
from pathlib import Path
import hashlib,json,os,signal,subprocess,tempfile
r=Path.cwd();e=Path(__file__).resolve().parent
for sig in [signal.SIGTERM,signal.SIGINT]:signal.signal(sig,lambda s,f: (_ for _ in ()).throw(SystemExit(128+s)))
def sha(p):
 with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
old=json.loads((r/'dist/packages/LINUX_PACKAGES_ACCEPTANCE.json').read_text());new=json.loads((e/'PACKAGE_CONTENT_ACCEPTANCE.json').read_text());name='Ottrin-0.1.0-x86_64.AppImage';previous=r/'dist/packages'/name;candidate=Path(json.loads((e/'BUILD_SOURCE.json').read_text())['stage'])/name
assert sha(previous)==old['artifacts'][name] and sha(candidate)==new['artifacts'][name]
def manifest(folder):
 return {str(p.relative_to(folder)):({'symlink':os.readlink(p)} if p.is_symlink() else {'sha256':sha(p),'mode':p.stat().st_mode&0o777}) for p in folder.rglob('*') if p.is_symlink() or p.is_file()}
with tempfile.TemporaryDirectory(prefix='.startup-identity-',dir=r/'.hermes/packaging-tmp') as directory:
 temporary=Path(directory);manifests=[]
 for label,image in [('previous',previous),('candidate',candidate)]:
  folder=temporary/label;folder.mkdir();subprocess.run([str(image),'--appimage-extract'],cwd=folder,stdout=subprocess.DEVNULL,check=True);manifests.append(manifest(folder/'squashfs-root'))
 before,after=manifests;
 import struct
 binaries=[(temporary/label/'squashfs-root/usr/bin/ottrin-indexd').read_bytes() for label in ['previous','candidate']]
 def elf(data):
  assert data[:6]==b'\x7fELF\x02\x01'
  offset=struct.unpack_from('<Q',data,32)[0]; size,count=struct.unpack_from('<HH',data,54); loads=[]
  for i in range(count):
   h=struct.unpack_from('<IIQQQQQQ',data,offset+i*size)
   if h[0]==1: loads.append((h,data[h[2]:h[2]+h[5]]))
  return loads
 x,y=map(elf,binaries);assert len(x)==len(y)
 differences=[]
 for (hx,dx),(hy,dy) in zip(x,y):
  assert hx==hy
  differences.extend(hx[2]+i for i,(a,b) in enumerate(zip(dx,dy)) if a!=b)
 # Identify the actual GNU build-id note through ELF section names, then allow
 # only that note's descriptor and the nonloaded section-table pointer to differ.
 def build_id(data):
  off=struct.unpack_from('<Q',data,40)[0];size,count,names=struct.unpack_from('<HHH',data,58)
  sh=[struct.unpack_from('<IIQQQQIIQQ',data,off+i*size) for i in range(count)]
  string=sh[names];table=data[string[4]:string[4]+string[5]]
  matches=[h for h in sh if table[h[0]:].split(b'\0')[0]==b'.note.gnu.build-id'];assert len(matches)==1
  h=matches[0];ns,ds,typ=struct.unpack_from('<III',data,h[4]);assert ns==4 and typ==3 and data[h[4]+12:h[4]+16]==b'GNU\0'
  return h[4]+16,ds
 ids=[build_id(d) for d in binaries];assert ids[0]==ids[1]
 allowed=set(range(40,48))|set(range(ids[0][0],sum(ids[0])))
 equal=set(differences)<=allowed
 identity={'status':'pass mapped-image identity' if equal else 'different mapped image: fresh runtime verification required','mapped_image_equal':equal,'whole_file_equal':binaries[0]==binaries[1],'versions':[{'sha256':hashlib.sha256(d).hexdigest(),'bytes':len(d)} for d in binaries],'different_loaded_byte_count':len(differences),'different_loaded_offsets_first64':differences[:64],'different_loaded_bytes_outside_metadata':len(set(differences)-allowed),'build_id_descriptor':{'offset':ids[0][0],'bytes':ids[0][1]},'scope':'Complete load-segment comparison, including code/data/dynamic dependencies. No prior daemon runtime or CPU acceptance is carried forward from binary identity when mapped_image_equal is false.'}
 (e/'LINUX_INDEX_ELF_COMPARISON.json').write_text(json.dumps(identity,indent=2)+'\n')
 assert before.keys()==after.keys();changed={name:{'previous':before[name],'candidate':after[name]} for name in before if before[name]!=after[name]}
 allowed={'usr/bin/ottrin','usr/bin/ottrin-indexd','usr/share/doc/ottrin/README.md','usr/share/doc/ottrin/USER_GUIDE.md','usr/share/doc/ottrin/ROADMAP.md','usr/share/doc/ottrin/RELEASE_NOTES.md','usr/share/doc/ottrin/SOURCE_REVISION.txt'}
 assert set(changed)<=allowed,changed
 assert {'usr/bin/ottrin','usr/share/doc/ottrin/SOURCE_REVISION.txt'}<=set(changed)

assert not temporary.exists()
(e/'FULL_LINUX_PAYLOAD_COMPARISON.json').write_text(json.dumps({'status':'pass full AppImage file/symlink/mode change inventory','previous_source':old['source_revision'],'candidate_source':new['source_revision'],'previous_artifact_sha256':old['artifacts'][name],'candidate_artifact_sha256':new['artifacts'][name],'entries':len(before),'changed':changed,'unchanged_entries':len(before)-len(changed),'scope':'Every runtime library, other helper, font, asset, launcher and policy unchanged. Daemon loaded-image differences are recorded in LINUX_INDEX_ELF_COMPARISON.json and require fresh scoped runtime checks; no binary-equivalence claim. GUI separately checked in native captures; earlier functional receipts remain historical.','temporary_extractions_removed':True},indent=2)+'\n')
