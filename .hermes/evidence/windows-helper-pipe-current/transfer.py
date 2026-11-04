"""Transfer exact native fixtures, retaining section identity and removing staging."""
from pathlib import Path
import gzip,hashlib,json,shutil,subprocess,sys,tempfile
sys.dont_write_bytecode=True
E=Path(__file__).resolve().parent;R=E.parents[2]
sys.path.insert(0,str(E));import transport as n
sys.path.insert(0,str(R/'.hermes/evidence/windows-copy-namespace-current'));from pe_sections import sections
rows=[json.loads(row) for row in (E/'build.jsonl').read_text().splitlines() if row.startswith('{')]
helper_rows=[json.loads(row) for row in (E/'helper-build.jsonl').read_text().splitlines() if row.startswith('{')]
paths={name:Path(next(row['executable'] for row in rows if row.get('reason')=='compiler-artifact' and row.get('executable') and row['target']['name']==name)) for name in ['ottrin_platform','ottrin_search']}
paths['helper']=Path(next(row['executable'] for row in helper_rows if row.get('reason')=='compiler-artifact' and row.get('executable') and row['target']['name']=='ottrin-priv-helper'))
receipt=json.loads((E/'TEST_BINARIES.json').read_text()) if (E/'TEST_BINARIES.json').exists() else {}
if sys.argv[1:]:
 assert set(sys.argv[1:]) <= set(paths)
 paths={name:paths[name] for name in sys.argv[1:]}
with tempfile.TemporaryDirectory(prefix='helper-pipe-transfer-',dir=R/'.hermes/packaging-tmp') as temporary:
 for name,source in paths.items():
  target=Path(temporary)/(name+'.exe');shutil.copyfile(source,target)
  subprocess.run(['x86_64-w64-mingw32-strip','--strip-debug',str(target)],check=True)
  assert sections(source)==sections(target)
  sha=hashlib.sha256(target.read_bytes()).hexdigest();receipt[name]={'compiler_path':str(source),'compiler_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'transferred_sha256':sha,'retained_sections':sections(target)}
  archive=Path(temporary)/(name+'.gz')
  with target.open('rb') as input,gzip.open(archive,'wb',compresslevel=1) as output:shutil.copyfileobj(input,output)
  n.upload(archive,archive.name)
  assert n.execute(r'''$root='C:\OttrinHelperPipeQA';$source=[IO.File]::OpenRead("$root\NAME.gz");$zip=[IO.Compression.GZipStream]::new($source,[IO.Compression.CompressionMode]::Decompress);$target=[IO.File]::Create("$root\NAME.exe");try{$zip.CopyTo($target)}finally{$target.Dispose();$zip.Dispose();$source.Dispose()};Remove-Item -LiteralPath "$root\NAME.gz";if((Get-FileHash "$root\NAME.exe").Hash.ToLowerInvariant()-ne'HASH'){throw 'Owned native fixture hash mismatch'}'''.replace('NAME',name).replace('HASH',sha))==0
 if 'helper' in paths:assert n.execute(r'''Copy-Item 'C:\OttrinHelperPipeQA\helper.exe' 'C:\OttrinHelperPipeQA\package space Å\ottrin-priv-helper.exe';Remove-Item C:\OttrinHelperPipeQA\helper.exe''')==0
receipt['host_transfer_stage_removed']=True
(E/'TEST_BINARIES.json').write_text(json.dumps(receipt,indent=2)+'\n')
