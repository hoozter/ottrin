from pathlib import Path
import json,base64
import native_compiler as n
p=Path(__file__).parent
script=r'''$r='C:\OttrinCIPackageQA';$p="$r\payload\Ottrin-0.1.0-windows-x64";$m=Get-Content "$r\PAYLOAD.json" -Raw|ConvertFrom-Json;$files=@(Get-ChildItem $p -File -Recurse);$entries=@($m.PSObject.Properties);if($files.Count -ne $entries.Count){throw "Payload count mismatch: $($files.Count) versus $($entries.Count)"};foreach($e in $entries){if((Get-FileHash (Join-Path $p $e.Name)).Hash.ToLowerInvariant()-ne$e.Value){throw "Payload mismatch: $($e.Name)"}};& "$r\dotnet\dotnet.exe" "$r\wix-package\tools\net6.0\any\wix.dll" --version;if($LASTEXITCODE -ne 0){throw 'Compiler validation failed'};@{status='pass';payloadFiles=$files.Count;scope='native exact portable extraction and compiler validation'}|ConvertTo-Json'''
r=n.wait(n.start(script));(p/'COMPILER_PREPARE_VALIDATED.json').write_text(json.dumps({'exit':r['exitcode'],'stdout':base64.b64decode(r.get('out-data','')).decode(),'correction':'Reused completed preparation after Python observer Path name error; corrected PSObject property enumeration before acceptance'},indent=2)+'\n')
