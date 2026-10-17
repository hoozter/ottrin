from pathlib import Path
import sys,json,hashlib
p=Path(__file__).resolve().parent;r=p.parents[2]
sys.path.insert(0,str(p.parent/'windows-copy-cleanup-current'));import transport as n
n.GROOT=r'C:\OttrinCopyCleanupQA\compiler'
archive=r/'.hermes/packaging-tmp/windows-activity-candidate/Ottrin-0.1.0-windows-x64.msi';expected=hashlib.sha256(archive.read_bytes()).hexdigest()
script=r'''$r='C:\OttrinCopyCleanupQA\compiler';$d='C:\OttrinCopyCleanupQA\desktop';if((Get-FileHash "$r\current.msi").Hash.ToLowerInvariant()-ne'EXPECTED'){throw 'MSI changed'};if(Test-Path "$r\msi-extract"){throw 'Extraction exists'};$proc=Start-Process msiexec.exe -ArgumentList @('/a',"$r\current.msi",'/qn',"TARGETDIR=$r\msi-extract",'/l*v',"$r\extract.log") -Wait -PassThru;if($proc.ExitCode-ne0){throw "Admin extraction exit $($proc.ExitCode)"};$roots=@(Get-ChildItem "$r\msi-extract" -Recurse -Filter SOURCE_REVISION.txt);if($roots.Count-ne1){throw 'Ambiguous payload'};$root=$roots[0].Directory.FullName;$m=Get-Content "$d\PAYLOAD.json" -Raw|ConvertFrom-Json;foreach($e in $m.entries.PSObject.Properties){$relative=$e.Name.Substring($e.Name.IndexOf('/')+1);if((Get-FileHash (Join-Path $root $relative)).Hash.ToLowerInvariant()-ne$e.Value){throw "Mismatch $relative"}};$count=@(Get-ChildItem $root -Recurse -File).Count;if($count-ne57){throw 'Unexpected payload files'};@{status='pass';source_revision=$m.source_commit;entries_checked=@($m.entries.PSObject.Properties).Count;payload_files=$count;msi_sha256='EXPECTED'}|ConvertTo-Json|Set-Content "$r\MSI_PAYLOAD.json";Get-Content "$r\MSI_PAYLOAD.json"'''.replace('EXPECTED',expected)
pid=n.start(script);(p/'verify-msi.pid').write_text(str(pid))
while True:
 try:n.wait(pid,45);break
 except TimeoutError:print('Observing same extraction PID',flush=True)
n.download('MSI_PAYLOAD.json',p/'MSI_PAYLOAD.json')
