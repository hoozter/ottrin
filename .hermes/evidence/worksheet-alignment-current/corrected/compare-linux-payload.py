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
with tempfile.TemporaryDirectory(prefix='.worksheet-identity-',dir=r/'.hermes/packaging-tmp') as directory:
 temporary=Path(directory);manifests=[]
 for label,image in [('previous',previous),('candidate',candidate)]:
  folder=temporary/label;folder.mkdir();subprocess.run([str(image),'--appimage-extract'],cwd=folder,stdout=subprocess.DEVNULL,check=True);manifests.append(manifest(folder/'squashfs-root'))
 before,after=manifests;assert before.keys()==after.keys();changed={name:{'previous':before[name],'candidate':after[name]} for name in before if before[name]!=after[name]}
 expected={'usr/bin/ottrin','usr/share/doc/ottrin/ROADMAP.md','usr/share/doc/ottrin/SOURCE_REVISION.txt'};assert changed.keys()==expected,changed
assert not temporary.exists()
(e/'FULL_LINUX_PAYLOAD_COMPARISON.json').write_text(json.dumps({'status':'pass full AppImage file/symlink/mode comparison','previous_source':old['source_revision'],'candidate_source':new['source_revision'],'previous_artifact_sha256':old['artifacts'][name],'candidate_artifact_sha256':new['artifacts'][name],'entries':len(before),'changed':changed,'unchanged_entries':len(before)-len(changed),'scope':'Every runtime library, helper, font, asset, launcher, policy and other documentation input unchanged. GUI change independently checked in native captures; roadmap/provenance differences expected. Prior UI scopes remain historical, not freshly rerun.','temporary_extractions_removed':True},indent=2)+'\n')
