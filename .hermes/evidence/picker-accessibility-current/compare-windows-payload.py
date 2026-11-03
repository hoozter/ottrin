"""Compare every shipped payload and the complete daemon PE, without executing it."""
from pathlib import Path
import hashlib,json,struct
r=Path.cwd();e=Path(__file__).resolve().parent
read=lambda p:json.loads(p.read_text())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
old=read(r/'dist/WINDOWS_CANDIDATE_ACCEPTANCE.json');build=read(e/'windows/BUILD_RESULT.json');a=r/'dist/windows-candidate/windows-payload/Ottrin-0.1.0-windows-x64';b=Path(build['stage'])/'windows-payload/Ottrin-0.1.0-windows-x64'
manifest=lambda p:{str(x.relative_to(p)):sha(x) for x in p.rglob('*') if x.is_file()}
x,y=manifest(a),manifest(b);assert len(x)==len(y)==58 and x.keys()==y.keys()
assert y==read(e/'PAYLOAD.json')
changed={n:{'previous_sha256':x[n],'candidate_sha256':y[n]} for n in x if x[n]!=y[n]}
assert set(changed)<=set(['SOURCE_REVISION.txt','docs/ROADMAP.md','docs/RELEASE_NOTES.md','bin/ottrin.exe','bin/ottrin-indexd.exe'])
versions=[];normalized=[];raw=[]
for p in [a/'bin/ottrin-indexd.exe',b/'bin/ottrin-indexd.exe']:
 data=p.read_bytes();raw.append(data);assert data[:2]==b'MZ';pe=struct.unpack_from('<I',data,60)[0];assert data[pe:pe+4]==b'PE\0\0';optional=pe+24;assert struct.unpack_from('<H',data,optional)[0]==0x20b
 offsets=[pe+8,optional+64];v={'path':str(p),'sha256':sha(p),'bytes':len(data),'timestamp_offset':offsets[0],'checksum_offset':offsets[1],'timestamp':struct.unpack_from('<I',data,offsets[0])[0],'checksum':struct.unpack_from('<I',data,offsets[1])[0]};copy=bytearray(data)
 for off in offsets:copy[off:off+4]=b'\0'*4
 normalized.append(copy);versions.append(v)
assert normalized[0]==normalized[1]
pe={'status':'pass: complete PE equal apart from COFF timestamp and optional-header checksum','versions':versions,'different_offsets':[i for i,(a,b) in enumerate(zip(*raw)) if a!=b],'normalized_whole_file_sha256':hashlib.sha256(normalized[0]).hexdigest(),'scope':'All code/data/resources/imports/unwind/relocation/debug and every other byte identical. Prior CPU/runtime receipts retain original hashes; no fresh benchmark claimed.'}
(e/'WINDOWS_INDEX_PE_COMPARISON.json').write_text(json.dumps(pe,indent=2)+'\n')
receipt={'status':'pass complete58-file comparison with exact daemon PE metadata-only difference','previous_source':old['source_commit'],'candidate_source':build['source_revision'],'entries':58,'changed':changed,'unchanged_entries':58-len(changed),'daemon_identity':'WINDOWS_INDEX_PE_COMPARISON.json','scope':'GUI independently requires native acceptance; documentation/provenance changes expected. All other helpers, libraries, fonts, icons and transaction payloads byte-identical.'}
(e/'FULL_WINDOWS_PAYLOAD_COMPARISON.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(receipt))
