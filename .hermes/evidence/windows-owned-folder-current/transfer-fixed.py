from pathlib import Path
import json,hashlib,subprocess,tempfile,sys,gzip,shutil,time
r=Path(__file__).resolve().parent;sys.path.insert(0,str(r));import transport as n
state=subprocess.check_output(['virsh','-c','qemu:///system','domstate',n.guest.DOMAIN],text=True).strip();assert state=='shut off',state
subprocess.run(['virsh','-c','qemu:///system','start',n.guest.DOMAIN],check=True,capture_output=True)
for attempt in range(80):
 try:n.qga('guest-ping');break
 except RuntimeError:
  assert attempt<79,'Observe same boot; do not restart';time.sleep(.5)
rows=[json.loads(l) for l in (r/'build-fixed.jsonl').read_text().splitlines()];original=Path(next(x['executable'] for x in rows if x.get('reason')=='compiler-artifact' and x.get('executable') and x.get('profile',{}).get('test') and x['target']['name']=='ottrin_platform'))
source={'base_revision':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'dirty':True,'tracked_delta':subprocess.check_output(['git','diff','--name-only'],text=True).splitlines(),'path':str(original),'sha256':hashlib.file_digest(original.open('rb'),'sha256').hexdigest(),'source_file_sha256':hashlib.sha256(Path('crates/ottrin-platform/src/move_transaction.rs').read_bytes()).hexdigest()}
assert source['tracked_delta']==['crates/ottrin-platform/src/move_transaction.rs'],source['tracked_delta']
sys.path.insert(0,str(r.parent/'windows-copy-namespace-current'));from pe_sections import sections
previous=json.loads((r/'TEST_BINARY.json').read_text())
with tempfile.TemporaryDirectory(prefix='owned-folder-fixed-',dir=r.parents[1]/'packaging-tmp') as t:
 exe=Path(t)/'platform.exe';subprocess.run(['x86_64-w64-mingw32-strip','--strip-debug','-o',str(exe),str(original)],check=True)
 assert sections(exe)==sections(original)
 z=Path(t)/'platform-fixed.gz'
 with exe.open('rb') as inp,gzip.open(z,'wb',compresslevel=1) as out:shutil.copyfileobj(inp,out)
 digest=hashlib.sha256(exe.read_bytes()).hexdigest();source.update(transferred_sha256=digest,transferred_bytes=exe.stat().st_size,retained_sections=sections(exe));(r/'FIXED_TEST_BINARY.json').write_text(json.dumps(source,indent=2)+'\n');n.upload(z,'platform-fixed.gz')
 n.execute(r'''$ErrorActionPreference='Stop'
$task=Get-ScheduledTask -TaskName 'Ottrin Owned Folder QA'
if($task.State-eq'Running'){throw 'Owned actor remains live'}
if((Get-FileHash C:\OttrinOwnedFolderQA\platform.exe).Hash.ToLowerInvariant()-ne'OLD'){throw 'Old owned test executable changed'}
$s=[IO.File]::OpenRead('C:\OttrinOwnedFolderQA\platform-fixed.gz');$z=[IO.Compression.GZipStream]::new($s,[IO.Compression.CompressionMode]::Decompress);$d=[IO.File]::Create('C:\OttrinOwnedFolderQA\platform-next.exe')
try{$z.CopyTo($d)}finally{$d.Dispose();$z.Dispose();$s.Dispose()}
if((Get-FileHash C:\OttrinOwnedFolderQA\platform-next.exe).Hash.ToLowerInvariant()-ne'NEW'){throw 'New native executable hash mismatch'}
Move-Item C:\OttrinOwnedFolderQA\platform-next.exe C:\OttrinOwnedFolderQA\platform.exe -Force
Remove-Item -LiteralPath C:\OttrinOwnedFolderQA\platform-fixed.gz
'''.replace('OLD',previous['transferred_sha256']).replace('NEW',digest))
n.upload(r/'actor.ps1','actor.ps1')
print(n.execute(r'''$ErrorActionPreference='Stop'
'elevated-fixed'|Set-Content C:\OttrinOwnedFolderQA\PHASE.txt
Start-ScheduledTask -TaskName 'Ottrin Owned Folder QA'
(Get-ScheduledTask -TaskName 'Ottrin Owned Folder QA').State.ToString()
''')['out-data'])
print('Fixed exact-source native tests dispatched; observe same owned scheduled task.')
