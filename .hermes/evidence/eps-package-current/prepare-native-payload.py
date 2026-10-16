from pathlib import Path
import json,hashlib,sys
p=Path(__file__).resolve().parent;sys.path.insert(0,str(p));import transport as n
archive=Path('.hermes/packaging-tmp/eps-refresh/windows/Ottrin-0.1.0-windows-x64-portable.zip');digest=hashlib.sha256(archive.read_bytes()).hexdigest()
assert n.execute(r'''if((Get-ScheduledTask 'Ottrin Content QA').State-eq'Running'){throw 'Test task live'};if(Test-Path C:\OttrinContentQA\payload){throw 'Inspect existing payload first'}''')==0
n.upload(archive,'payload.zip')
pid=n.start(r'''$root='C:\OttrinContentQA';if((Get-FileHash "$root\payload.zip").Hash.ToLowerInvariant()-ne'HASH'){throw 'Payload archive identity mismatch'};Expand-Archive "$root\payload.zip" "$root\payload";Remove-Item "$root\payload.zip"'''.replace('HASH',digest));(p/'NATIVE_PAYLOAD_PID.json').write_text(json.dumps({'pid':pid})+'\n');n.wait(pid,45)
