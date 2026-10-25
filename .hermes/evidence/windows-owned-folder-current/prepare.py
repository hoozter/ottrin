from pathlib import Path
import json,hashlib,subprocess,tempfile,sys,gzip,shutil
r=Path(__file__).resolve().parent;sys.path.insert(0,str(r));import transport as n
source=json.loads((r/'TEST_BINARY.json').read_text());owner='ottrin-owned-folder-'+source['source']
setup=r'''$ErrorActionPreference='Stop'
$root='C:\OttrinOwnedFolderQA'
$user='OttrinOwnedFolderQA'
if(Test-Path -LiteralPath $root){throw 'Task root already exists; inspect ownership'}
if(Get-LocalUser -Name $user -ErrorAction SilentlyContinue){throw 'Task account already exists'}
if(Get-ScheduledTask -TaskName 'Ottrin Owned Folder QA' -ErrorAction SilentlyContinue){throw 'Task already exists'}
New-Item -ItemType Directory $root|Out-Null
'OWNER'|Set-Content -LiteralPath "$root\OWNER.txt"
$baseline=@{disks=@(Get-Disk|Select-Object Number,UniqueId,Size);originalProfiles=@(Get-CimInstance Win32_UserProfile|Select-Object SID,LocalPath);originalUsers=@(Get-LocalUser|Select-Object Name,SID)}
$baseline|ConvertTo-Json -Depth 6|Set-Content "$root\BASELINE.json"
$password=[Guid]::NewGuid().ToString('N')+'!aA7'
$secure=ConvertTo-SecureString $password -AsPlainText -Force
$account=New-LocalUser -Name $user -Password $secure -AccountNeverExpires -PasswordNeverExpires
Add-LocalGroupMember -Group (Get-LocalGroup -SID 'S-1-5-32-545').Name -Member $account
$group=(Get-LocalGroup -SID 'S-1-5-32-544').Name
Add-LocalGroupMember -Group $group -Member $account
& icacls.exe $root /inheritance:r /grant:r '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' ('*'+$account.SID.Value+':(OI)(CI)F')|Out-Null
if($LASTEXITCODE-ne0){throw 'Task root ACL failed'}
New-Item -ItemType Directory "$root\results"|Out-Null
$action=New-ScheduledTaskAction -Execute 'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe' -Argument '-NoProfile -NonInteractive -ExecutionPolicy Bypass -File C:\OttrinOwnedFolderQA\actor.ps1'
$principal=New-ScheduledTaskPrincipal -UserId $account.SID.Value -LogonType Password -RunLevel Highest
$task=New-ScheduledTask -Action $action -Principal $principal -Settings (New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Minutes 8))
Register-ScheduledTask -TaskName 'Ottrin Owned Folder QA' -InputObject $task -User $user -Password $password|Out-Null
@{status='prepared';sid=$account.SID.Value;owner='OWNER'}|ConvertTo-Json|Set-Content "$root\OWNER.json"
'Prepared owned elevated native fixture'
'''.replace("'OWNER'","'"+owner+"'")
n.execute(setup)
n.download('BASELINE.json',r/'GUEST_BASELINE.json');n.download('OWNER.json',r/'GUEST_OWNER.json')
sys.path.insert(0,str(r.parent/'windows-copy-namespace-current'));from pe_sections import sections
with tempfile.TemporaryDirectory(prefix='owned-folder-binary-',dir=r.parents[1]/'packaging-tmp') as t:
 exe=Path(t)/'platform.exe';subprocess.run(['x86_64-w64-mingw32-strip','--strip-debug','-o',str(exe),source['path']],check=True)
 assert sections(exe)==sections(Path(source['path']))
 z=Path(t)/'platform.gz'
 with exe.open('rb') as inp,gzip.open(z,'wb',compresslevel=1) as out:shutil.copyfileobj(inp,out)
 digest=hashlib.sha256(exe.read_bytes()).hexdigest();source.update(transferred_sha256=digest,transferred_bytes=exe.stat().st_size,retained_sections=sections(exe));(r/'TEST_BINARY.json').write_text(json.dumps(source,indent=2)+'\n');n.upload(z,'platform.gz')
 n.execute(r'''$ErrorActionPreference='Stop'
$s=[IO.File]::OpenRead('C:\OttrinOwnedFolderQA\platform.gz');$z=[IO.Compression.GZipStream]::new($s,[IO.Compression.CompressionMode]::Decompress);$d=[IO.File]::Create('C:\OttrinOwnedFolderQA\platform.exe')
try{$z.CopyTo($d)}finally{$d.Dispose();$z.Dispose();$s.Dispose()}
Remove-Item -LiteralPath 'C:\OttrinOwnedFolderQA\platform.gz'
if((Get-FileHash C:\OttrinOwnedFolderQA\platform.exe).Hash.ToLowerInvariant()-ne'HASH'){throw 'Transferred native executable hash mismatch'}
'''.replace('HASH',digest))
print('Current exact native test executable prepared; temporary host binary removed.')
