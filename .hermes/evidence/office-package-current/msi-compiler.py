#!/usr/bin/env python3
from pathlib import Path
import sys,json,hashlib,base64,os
p=Path(__file__).resolve().parent;root=p.parents[2]
sys.path.insert(0,str(p.parent/'windows-copy-cleanup-current'));import transport as n
n.GROOT=r'C:\OttrinCopyCleanupQA\compiler'
def wait(pid):
 (p/'msi-compiler.pid').write_text(str(pid))
 while True:
  try:return n.wait(pid,45)
  except TimeoutError:print('Compiler remains live; observing same PID',flush=True)
if sys.argv[1:]==['prepare']:
 pins=json.loads((p.parent/'windows-20261011/windows-msi-compiler-downloads.json').read_text())
 cache=root/'.hermes/tools/windows-msi-native'
 assert hashlib.sha512((cache/'runtime.zip').read_bytes()).hexdigest()==pins['runtime_sha512']
 assert hashlib.sha256((cache/'wix.nupkg').read_bytes()).hexdigest()==pins['wix_sha256']
 wait(n.start(r"$r='C:\OttrinCopyCleanupQA\compiler';if(Test-Path $r){throw 'Compiler root already exists; inspect before reuse'};New-Item -ItemType Directory $r|Out-Null"))
 n.upload(cache/'runtime.zip','runtime.zip');n.upload(cache/'wix.nupkg','wix.zip')
 wait(n.start(r'''$r='C:\OttrinCopyCleanupQA\compiler';Expand-Archive "$r\runtime.zip" "$r\dotnet";Expand-Archive "$r\wix.zip" "$r\wix";& "$r\dotnet\dotnet.exe" "$r\wix\tools\net6.0\any\wix.dll" --version;if($LASTEXITCODE-ne0){throw 'Compiler failed'};Remove-Item "$r\runtime.zip","$r\wix.zip"'''))
else:
 args=sys.argv[1:];dist=Path(os.environ['OTTRIN_DIST_DIR']).resolve();stage=dist/'windows-payload/Ottrin-0.1.0-windows-x64'
 assert len(args)==10 and args[:3]==['build','-arch','x64'] and args[4]=='Version=0.1.0' and args[6]=='Stage='+str(stage),args
 assert Path(args[9])==root/'packaging/windows/installer.wxs'
 output=Path(args[8]);assert output.parent.parent==dist and output.parent.name.startswith('.windows-installer.')
 n.upload(Path(args[9]),'installer.wxs')
 result=wait(n.start(r'''$r='C:\OttrinCopyCleanupQA\compiler';$desktop='C:\OttrinCopyCleanupQA\desktop';$m=Get-Content "$desktop\PAYLOAD.json" -Raw|ConvertFrom-Json;foreach($e in $m.entries.PSObject.Properties){if((Get-FileHash (Join-Path "$desktop\payload" $e.Name)).Hash.ToLowerInvariant()-ne$e.Value){throw 'Compiler payload changed'}};& "$r\dotnet\dotnet.exe" "$r\wix\tools\net6.0\any\wix.dll" build -arch x64 -d Version=0.1.0 -d 'Stage=C:\OttrinCopyCleanupQA\desktop\payload\Ottrin-0.1.0-windows-x64' -o "$r\current.msi" "$r\installer.wxs";if($LASTEXITCODE-ne0){throw 'MSI compilation failed'};(Get-FileHash "$r\current.msi").Hash.ToLowerInvariant()'''))
 expected=base64.b64decode(result['out-data']).decode().strip().splitlines()[-1]
 n.download('current.msi',output)
 assert hashlib.sha256(output.read_bytes()).hexdigest()==expected
 print('Native MSI SHA256',expected,flush=True)
