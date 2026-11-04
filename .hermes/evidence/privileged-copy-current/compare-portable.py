"""Compare accepted/staged portable payload without creating an extraction."""
from pathlib import Path
import hashlib,json,tarfile
r=Path.cwd();e=Path(__file__).resolve().parent
build=json.loads((e/'BUILD_RESULT.json').read_text());previous=json.loads((e/'PREVIOUS_LINUX_PACKAGES_ACCEPTANCE.json').read_text());name='Ottrin-0.1.0-linux-x86_64-portable.tar.gz'
def payload(path,expected):
 with path.open('rb') as f:assert hashlib.file_digest(f,'sha256').hexdigest()==expected
 with tarfile.open(path) as archive:
  values={}
  for member in archive.getmembers():
   if member.isfile():
    name='/'.join(Path(member.name).parts[1:]);assert name and name not in values
    stream=archive.extractfile(member);values[name]=hashlib.file_digest(stream,'sha256').hexdigest()
 return values
old=payload(r/'dist/packages'/name,previous['artifacts'][name]);new=payload(Path(build['stage'])/name,build['artifacts'][name]);assert old.keys()==new.keys()
changes={name:{'previous':old[name],'current':new[name]} for name in new if old[name]!=new[name]}
allowed={'SOURCE_REVISION.txt','libexec/ottrin/ottrin','libexec/ottrin/ottrin-priv-helper','libexec/ottrin/ottrin-indexd','README.md','docs/ROADMAP.md','docs/RELEASE_NOTES.md'}
assert set(changes)<=allowed,changes
report={'status':'pass exact portable payload comparison','source_revision':build['source_revision'],'previous_source_revision':previous['source_revision'],'payload_files':len(new),'changes':changes,'daemon_byte_identical':old['libexec/ottrin/ottrin-indexd']==new['libexec/ottrin/ottrin-indexd'],'daemon_sha256':new['libexec/ottrin/ottrin-indexd'],'scope':'Only GUI, privileged helper, rebuilt daemon, provenance and manual snapshots change. Media/preview helpers and dependencies remain byte-identical. Changed daemon requires fresh runtime evidence; no prior CPU sample is carried forward.'}
report['fresh_daemon_runtime_required']=not report['daemon_byte_identical']
(e/'PAYLOAD_COMPARISON.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
