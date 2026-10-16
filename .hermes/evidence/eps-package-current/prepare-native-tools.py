from pathlib import Path
import hashlib,json,sys
p=Path(__file__).resolve().parent;sys.path.insert(0,str(p));import transport as n
cache=Path('.hermes/tools/windows-msi-native');pins=json.loads(Path('.hermes/evidence/windows-20261011/windows-msi-compiler-downloads.json').read_text())
assert hashlib.sha512((cache/'runtime.zip').read_bytes()).hexdigest()==pins['runtime_sha512']
assert hashlib.sha256((cache/'wix.nupkg').read_bytes()).hexdigest()==pins['wix_sha256']
assert n.execute(r'''if((Get-ScheduledTask 'Ottrin Content QA').State-eq'Running'){throw 'Test task live'};foreach($name in @('dotnet','wix-package','runtime.zip','wix.zip')){if(Test-Path "C:\OttrinContentQA\$name"){throw "Inspect existing owned tool first: $name"}}''')==0
n.upload(cache/'runtime.zip','runtime.zip');n.upload(cache/'wix.nupkg','wix.zip')
pid=n.start(r'''$root='C:\OttrinContentQA';Expand-Archive "$root\runtime.zip" "$root\dotnet";Expand-Archive "$root\wix.zip" "$root\wix-package";& "$root\dotnet\dotnet.exe" "$root\wix-package\tools\net6.0\any\wix.dll" --version;if($LASTEXITCODE-ne0){exit $LASTEXITCODE};Remove-Item "$root\runtime.zip","$root\wix.zip"''')
(p/'NATIVE_TOOLS_PID.json').write_text(json.dumps({'pid':pid})+'\n');n.wait(pid,45)
(p/'NATIVE_TOOLS.json').write_text(json.dumps({'status':'pass','pins':pins,'scope':'Verified cached compiler/runtime in disposable owned COW guest; no installation or package acceptance'},indent=2)+'\n')
