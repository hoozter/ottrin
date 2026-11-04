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
assert set(changed)<=set(['SOURCE_REVISION.txt','README.md','docs/USER_GUIDE.md','docs/ROADMAP.md','docs/RELEASE_NOTES.md','bin/ottrin.exe','bin/ottrin-indexd.exe'])
versions=[];normalized=[];raw=[];sections=[]
for p in [a/'bin/ottrin-indexd.exe',b/'bin/ottrin-indexd.exe']:
 data=p.read_bytes();raw.append(data);assert data[:2]==b'MZ';pe=struct.unpack_from('<I',data,60)[0];assert data[pe:pe+4]==b'PE\0\0';optional=pe+24;assert struct.unpack_from('<H',data,optional)[0]==0x20b
 symbol_offset,symbol_count=struct.unpack_from('<II',data,pe+12);string_start=symbol_offset+symbol_count*18;string_size=struct.unpack_from('<I',data,string_start)[0];assert string_start+string_size==len(data)
 offsets=[pe+8,optional+64];copy=bytearray(data[:symbol_offset])
 for off in offsets:copy[off:off+4]=b'\0'*4
 normalized.append(copy)
 section_offset=optional+struct.unpack_from('<H',data,pe+20)[0];table={}
 for i in range(struct.unpack_from('<H',data,pe+6)[0]):
  off=section_offset+i*40;name=data[off:off+8].rstrip(b'\0').decode();size,start=struct.unpack_from('<II',data,off+16);table[name]={'offset':start,'bytes':size,'sha256':hashlib.sha256(data[start:start+size]).hexdigest()}
 sections.append(table);versions.append({'path':str(p),'sha256':sha(p),'bytes':len(data),'coff_symbol_offset':symbol_offset,'coff_symbol_count':symbol_count,'coff_string_bytes':string_size})
equal=normalized[0]==normalized[1] and sections[0]==sections[1]
pe={'status':'pass mapped-image identity' if equal else 'different mapped image: fresh runtime verification required','mapped_image_equal':equal,'versions':versions,'all_sections':sections,'normalized_mapped_file_sha256':[hashlib.sha256(v).hexdigest() for v in normalized],'scope':'Complete section comparison, including code/data/resources/imports/unwind/relocation. No prior daemon runtime or CPU acceptance is carried forward from binary identity when mapped_image_equal is false.'}
(e/'WINDOWS_INDEX_PE_COMPARISON.json').write_text(json.dumps(pe,indent=2)+'\n')
receipt={'status':'pass complete58-file change inventory','previous_source':old['source_commit'],'candidate_source':build['source_revision'],'entries':58,'changed':changed,'unchanged_entries':58-len(changed),'daemon_comparison':'WINDOWS_INDEX_PE_COMPARISON.json','scope':'GUI independently requires native acceptance; documentation/provenance changes expected. Daemon complete mapped-image identity/differences are recorded separately; reuse requires mapped_image_equal. All other helpers, libraries, fonts, icons and transaction payloads byte-identical.'}
(e/'FULL_WINDOWS_PAYLOAD_COMPARISON.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(receipt))
