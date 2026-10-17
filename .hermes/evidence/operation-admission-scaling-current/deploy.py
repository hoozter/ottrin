from pathlib import Path
import hashlib,json,sys,zipfile
p=Path(__file__).resolve().parent
sys.path.insert(0,str(p.parent/'windows-copy-cleanup-current'))
import transport as n
n.GROOT=r'C:\OttrinCopyCleanupQA\desktop'
root=p.parents[2]
archive=root/'.hermes/packaging-tmp/windows-activity-candidate/Ottrin-0.1.0-windows-x64-portable.zip'
with zipfile.ZipFile(archive) as z:
 entries={i.filename:hashlib.sha256(z.read(i)).hexdigest() for i in z.infolist() if not i.is_dir()}
 revision=z.read('Ottrin-0.1.0-windows-x64/SOURCE_REVISION.txt').decode()
 assert '711706771cd7e32d0e3f422282275f010c27b3e8' in revision
 assert 'Working tree: clean' in revision,revision
manifest={'source_commit':'711706771cd7e32d0e3f422282275f010c27b3e8','archive_sha256':hashlib.sha256(archive.read_bytes()).hexdigest(),'app_sha256':entries['Ottrin-0.1.0-windows-x64/bin/ottrin.exe'],'entries':entries}
(p/'PAYLOAD.json').write_text(json.dumps(manifest,indent=2)+'\n')
n.upload(archive,'candidate.zip')
n.upload(p/'PAYLOAD.json','PAYLOAD.json')
script=r'''$root='C:\OttrinCopyCleanupQA\desktop';if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like"$root\payload\*"}).Count){throw 'Owned payload still live'};$m=Get-Content "$root\PAYLOAD.json" -Raw|ConvertFrom-Json;if((Get-FileHash "$root\candidate.zip").Hash.ToLowerInvariant()-ne$m.archive_sha256){throw 'Archive changed'};if(Test-Path "$root\payload"){Remove-Item "$root\payload" -Recurse -Force};Expand-Archive "$root\candidate.zip" "$root\payload";foreach($e in $m.entries.PSObject.Properties){if((Get-FileHash (Join-Path "$root\payload" $e.Name)).Hash.ToLowerInvariant()-ne$e.Value){throw "Payload mismatch: $($e.Name)"}};Remove-Item "$root\candidate.zip";@{source_commit=$m.source_commit;archive_sha256=$m.archive_sha256;entries_checked=@($m.entries.PSObject.Properties).Count}|ConvertTo-Json|Set-Content "$root\results\DEPLOYMENT.json";Get-Content "$root\results\DEPLOYMENT.json"'''
pid=n.start(script)
(p/'deploy.pid').write_text(str(pid))
n.wait(pid,45)
n.download('results\\DEPLOYMENT.json',p/'DEPLOYMENT.json')
