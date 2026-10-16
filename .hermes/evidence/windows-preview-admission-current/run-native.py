from pathlib import Path
import gzip,hashlib,json,shutil,subprocess,sys,tempfile
p=Path(__file__).resolve().parent;sys.path.insert(0,str(p));import transport as n
from pe_sections import sections
rows=[json.loads(x) for x in (p/'build.jsonl').read_text().splitlines()];pins={}
with tempfile.TemporaryDirectory(prefix='windows-preview-tests-',dir='.hermes/packaging-tmp') as temporary:
 for label,target in [('preview','ottrin_preview'),('ui','ottrin_ui')]:
  original=Path(next(x['executable'] for x in rows if x.get('reason')=='compiler-artifact' and x.get('target',{}).get('name')==target and x.get('profile',{}).get('test') and x.get('executable')))
  exe=Path(temporary)/(label+'.exe');shutil.copyfile(original,exe);subprocess.run(['x86_64-w64-mingw32-strip','--strip-debug',str(exe)],check=True);assert sections(exe)==sections(original)
  digest=hashlib.sha256(exe.read_bytes()).hexdigest();pins[label]={'compiler_path':str(original),'compiler_sha256':hashlib.sha256(original.read_bytes()).hexdigest(),'transferred_sha256':digest,'retained_sections':sections(exe)}
  archive=Path(temporary)/(label+'.gz')
  with exe.open('rb') as source,gzip.open(archive,'wb',compresslevel=1) as dest:shutil.copyfileobj(source,dest)
  n.upload(archive,label+'.gz')
  assert n.execute(r'''$s=[IO.File]::OpenRead('C:\OttrinContentQA\NAME.gz');$z=[IO.Compression.GZipStream]::new($s,[IO.Compression.CompressionMode]::Decompress);$d=[IO.File]::Create('C:\OttrinContentQA\NAME.exe');try{$z.CopyTo($d)}finally{$d.Dispose();$z.Dispose();$s.Dispose()};Remove-Item C:\OttrinContentQA\NAME.gz;if((Get-FileHash C:\OttrinContentQA\NAME.exe).Hash.ToLowerInvariant()-ne'HASH'){throw 'Native test hash mismatch'}'''.replace('NAME',label).replace('HASH',digest))==0
(p/'TEST_BINARIES.json').write_text(json.dumps(pins,indent=2)+'\n')
n.upload(p/'actor.ps1','actor.ps1')
assert n.execute(r'''$user=Get-LocalUser OttrinContentQA;$principal=New-ScheduledTaskPrincipal -UserId $user.SID.Value -LogonType Interactive -RunLevel Limited;$action=New-ScheduledTaskAction -Execute C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe -Argument '-NoProfile -ExecutionPolicy Bypass -File C:\OttrinContentQA\actor.ps1';if(Get-ScheduledTask 'Ottrin Content QA' -ErrorAction SilentlyContinue){if((Get-ScheduledTask 'Ottrin Content QA').State-eq'Running'){throw 'Actor still live'};Set-ScheduledTask -TaskName 'Ottrin Content QA' -Action $action|Out-Null}else{Register-ScheduledTask -TaskName 'Ottrin Content QA' -Principal $principal -Action $action|Out-Null};Start-ScheduledTask 'Ottrin Content QA';(Get-ScheduledTask 'Ottrin Content QA').State.ToString()''')==0
