from pathlib import Path
import sys,json,subprocess,time,hashlib,tempfile,gzip,shutil
p=Path(__file__).resolve().parent;sys.path.insert(0,str(p));import transport as n
for attempt in range(80):
 try:n.qga('guest-ping');break
 except RuntimeError:
  assert attempt<79;time.sleep(.5)
assert (p/'BASELINE.json').exists()
expected_baseline=hashlib.sha256((p/'BASELINE.json').read_bytes()).hexdigest()
n.execute(r"$ErrorActionPreference='Stop';if((Get-FileHash C:\OttrinOrdinaryQA\BASELINE.json).Hash.ToLowerInvariant()-ne'EXPECTED'){throw 'Fixture baseline changed'};if(Get-LocalUser -Name 'ottrin-ordinary-qa' -ErrorAction SilentlyContinue){throw 'Actor already provisioned; observe instead of restarting'}".replace('EXPECTED',expected_baseline))
prior=json.loads((p.parent/'windows-owned-folder-current/FIXED_TEST_BINARY.json').read_text());binary=Path(prior['path']);assert hashlib.file_digest(binary.open('rb'),'sha256').hexdigest()==prior['sha256'];assert hashlib.sha256(Path('crates/ottrin-platform/src/move_transaction.rs').read_bytes()).hexdigest()==prior['source_file_sha256']
sys.path.insert(0,str(p.parent/'windows-copy-namespace-current'));from pe_sections import sections
with tempfile.TemporaryDirectory(prefix='ordinary-native-transfer-',dir=p.parents[1]/'packaging-tmp') as t:
 exe=Path(t)/'platform.exe';subprocess.run(['x86_64-w64-mingw32-strip','--strip-debug','-o',str(exe),str(binary)],check=True);assert sections(exe)==sections(binary);digest=hashlib.file_digest(exe.open('rb'),'sha256').hexdigest()
 z=Path(t)/'platform.gz'
 with exe.open('rb') as source,gzip.open(z,'wb',compresslevel=1) as out:shutil.copyfileobj(source,out)
 n.upload(z,'platform.gz');n.execute(r'''$ErrorActionPreference='Stop';$r='C:\OttrinOrdinaryQA';$s=[IO.File]::OpenRead("$r\platform.gz");$z=[IO.Compression.GZipStream]::new($s,[IO.Compression.CompressionMode]::Decompress);$d=[IO.File]::Create("$r\platform.exe");try{$z.CopyTo($d)}finally{$d.Dispose();$z.Dispose();$s.Dispose()};if((Get-FileHash "$r\platform.exe").Hash.ToLowerInvariant()-ne'HASH'){throw 'Native binary changed'};Remove-Item "$r\platform.gz"'''.replace('HASH',digest))
 (p/'BINARY.json').write_text(json.dumps({'source_revision':'a230fa2c7b8f8389bd9ddf3a33331388f4febdef','source_sha256':prior['source_file_sha256'],'original_test_sha256':prior['sha256'],'transferred_sha256':digest,'kept_sections_equal':True,'prior_stripped_hash':prior['transferred_sha256'],'strip_metadata_changed':digest!=prior['transferred_sha256'],'temporary_transfer_removed_on_return':True},indent=2)+'\n')
for name in ['actor.ps1','controller.ps1']:n.upload(p/name,name)
pid=n.start(r"& 'C:\OttrinOrdinaryQA\controller.ps1'");(p/'CONTROLLER_PID.json').write_text(json.dumps({'pid':pid,'observe_same_pid':True})+'\n');print('Direct ordinary actor dispatched; controller PID',pid,flush=True)
