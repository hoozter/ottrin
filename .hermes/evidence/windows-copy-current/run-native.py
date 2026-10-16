from pathlib import Path
import gzip,hashlib,json,shutil,struct,subprocess,sys,tempfile
p=Path(__file__).resolve().parent;sys.path.insert(0,str(p));import transport as n
from pe_sections import sections
rows=[json.loads(x) for x in (p/'build.jsonl').read_text().splitlines()]
original=Path(next(x['executable'] for x in rows if x.get('reason')=='compiler-artifact' and x.get('executable') and x.get('profile',{}).get('test')))
with tempfile.TemporaryDirectory(prefix='windows-copy-tests-',dir='.hermes/packaging-tmp') as temp:
 exe=Path(temp)/'platform.exe';shutil.copyfile(original,exe);subprocess.run(['x86_64-w64-mingw32-strip','--strip-debug',str(exe)],check=True);assert sections(exe)==sections(original)
 digest=hashlib.sha256(exe.read_bytes()).hexdigest();(p/'TEST_BINARY.json').write_text(json.dumps({'compiler_path':str(original),'compiler_sha256':hashlib.sha256(original.read_bytes()).hexdigest(),'transferred_sha256':digest,'retained_sections':sections(exe)},indent=2)+'\n')
 z=Path(temp)/'platform.gz'
 with exe.open('rb') as source,gzip.open(z,'wb',compresslevel=1) as dest:shutil.copyfileobj(source,dest)
 n.upload(z,'platform.gz')
 assert n.execute(r'''$s=[IO.File]::OpenRead('C:\OttrinContentQA\platform.gz');$z=[IO.Compression.GZipStream]::new($s,[IO.Compression.CompressionMode]::Decompress);$d=[IO.File]::Create('C:\OttrinContentQA\platform.exe');try{$z.CopyTo($d)}finally{$d.Dispose();$z.Dispose();$s.Dispose()};Remove-Item C:\OttrinContentQA\platform.gz;if((Get-FileHash C:\OttrinContentQA\platform.exe).Hash.ToLowerInvariant()-ne'HASH'){throw 'Native test hash mismatch'}'''.replace('HASH',digest))==0
actor=p/'actor.ps1';actor.write_text(r'''$ErrorActionPreference='Stop';$result=@{status='running'};$p=$null;$temp=$null
try {
 if($env:USERNAME-ne'OttrinContentQA'){throw 'Wrong test account'}
 $identity=[Security.Principal.WindowsIdentity]::GetCurrent();if(([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Elevated test account'}
 $result.ordinary_user=$true;$temp=Join-Path $env:TEMP 'op-WindowsCopy';if(Test-Path $temp){throw 'Owned namespace exists'};New-Item -ItemType Directory $temp|Out-Null;$env:TEMP=$temp;$env:TMP=$temp
 $p=Start-Process C:\OttrinContentQA\platform.exe -ArgumentList 'copy_transaction::windows::tests --nocapture --test-threads=1' -WorkingDirectory C:\Windows\System32 -PassThru -RedirectStandardOutput C:\OttrinContentQA\results\platform.stdout -RedirectStandardError C:\OttrinContentQA\results\platform.stderr
 $null=$p.Handle;if(-not$p.WaitForExit(60000)){throw "Owned test remains live: $($p.Id)"}
 $result.exit=$p.ExitCode;if($p.ExitCode-ne0){throw 'Native stream tests failed'}
 $result.temporary_entries=@(Get-ChildItem -LiteralPath $temp -Force).Count;if($result.temporary_entries){throw 'Tests left temporary data'}
 $result.status='pass'
} catch {$result.status='fail';$result.error=$_.Exception.Message}
finally {
 if($p-and-not$p.HasExited){& C:\Windows\System32\taskkill.exe /PID $p.Id /T /F|Out-Null;$null=$p.WaitForExit(10000)}
 if($temp-and(Test-Path $temp)){Remove-Item -LiteralPath $temp -Recurse -Force}
 $result|ConvertTo-Json|Set-Content C:\OttrinContentQA\results\RESULTS.json
}
if($result.status-ne'pass'){exit 1}
''');n.upload(actor,actor.name)
assert n.execute(r'''$user=Get-LocalUser OttrinContentQA;$principal=New-ScheduledTaskPrincipal -UserId $user.SID.Value -LogonType Interactive -RunLevel Limited;$action=New-ScheduledTaskAction -Execute C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe -Argument '-NoProfile -ExecutionPolicy Bypass -File C:\OttrinContentQA\actor.ps1';if(Get-ScheduledTask 'Ottrin Content QA' -ErrorAction SilentlyContinue){if((Get-ScheduledTask 'Ottrin Content QA').State-eq'Running'){throw 'Actor still live'};Set-ScheduledTask -TaskName 'Ottrin Content QA' -Action $action|Out-Null}else{Register-ScheduledTask -TaskName 'Ottrin Content QA' -Principal $principal -Action $action|Out-Null};Start-ScheduledTask 'Ottrin Content QA';(Get-ScheduledTask 'Ottrin Content QA').State.ToString()''')==0
print('Native Copy staging tests dispatched')
