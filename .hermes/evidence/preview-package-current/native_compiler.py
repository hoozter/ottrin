#!/usr/bin/env python3
"""Owned-QA native compiler transport; never writes outside the scoped guest root."""
import base64,hashlib,json,os,pathlib,sys,time,subprocess
ROOT=pathlib.Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT/'.hermes/evidence/windows-20261011'))
import guest_exec as guest
guest.DOMAIN='ottrin-init-preview-package-qa'
qga=guest.qga
GROOT=r'C:\OttrinContentQA'
def execute(script):
    return guest.execute("$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'\n"+script,timeout=50)
def upload(source,name):
    source=source.resolve(strict=True)
    assert name and all(c.isalnum() or c in '-_.' for c in name)
    handle=qga('guest-file-open',{'path':GROOT+'\\'+name,'mode':'wb'})
    try:
        with source.open('rb') as stream:
            while block:=stream.read(48*1024):
                result=qga('guest-file-write',{'handle':handle,'buf-b64':base64.b64encode(block).decode()});assert result['count']==len(block)
        qga('guest-file-flush',{'handle':handle})
    finally:qga('guest-file-close',{'handle':handle})
    print('uploaded',name,source.stat().st_size,flush=True)
def download(name,output):
    h=qga('guest-file-open',{'path':GROOT+'\\'+name,'mode':'rb'})
    try:
        with output.open('wb') as stream:
            while True:
                block=qga('guest-file-read',{'handle':h,'count':65536})
                if block.get('buf-b64'):stream.write(base64.b64decode(block['buf-b64']))
                if block.get('eof'):break
    finally:qga('guest-file-close',{'handle':h})
def start(script):
    return qga('guest-exec',{'path':r'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe','arg':['-NoProfile','-NonInteractive','-EncodedCommand',base64.b64encode(("$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'\n"+script).encode('utf-16le')).decode()],'capture-output':True})['pid']
def wait(pid,seconds=600):
    print('native PID',pid,flush=True);deadline=time.monotonic()+seconds
    while True:
        result=qga('guest-exec-status',{'pid':pid})
        if result.get('exited'):
            for key in ['out-data','err-data']:
                if result.get(key):print(base64.b64decode(result[key]).decode(errors='replace'),flush=True)
            assert result.get('exitcode')==0,result
            return result
        if time.monotonic()>deadline:raise TimeoutError(f'Native PID {pid} remains live; poll same PID')
        time.sleep(.5)
def build(args):
    dist=pathlib.Path(os.environ['OTTRIN_DIST_DIR']).resolve();stage=dist/'windows-payload/Ottrin-0.1.0-windows-x64'
    assert len(args)==10 and args[:3]==['build','-arch','x64'] and args[4]=='Version=0.1.0' and args[6]=='Stage='+str(stage),args
    source=pathlib.Path(args[9]);assert source==ROOT/'packaging/windows/installer.wxs'
    output=pathlib.Path(args[8]);assert output.parent.parent==dist and output.parent.name.startswith('.windows-installer.')
    expected_manifest={p.relative_to(stage).as_posix():hashlib.sha256(p.read_bytes()).hexdigest() for p in stage.rglob('*') if p.is_file()}
    expected_file=pathlib.Path(__file__).parent/'compiler-payload.json';expected_file.write_text(json.dumps(expected_manifest));upload(expected_file,'compiler-payload.json')
    upload(source,source.name)
    result=wait(start(r'''$root='C:\OttrinContentQA'
$manifest=Get-Content "$root\compiler-payload.json" -Raw|ConvertFrom-Json
foreach($entry in $manifest.PSObject.Properties){if((Get-FileHash (Join-Path "$root\payload\Ottrin-0.1.0-windows-x64" $entry.Name)).Hash.ToLowerInvariant()-cne$entry.Value){throw "Compiler input differs: $($entry.Name)"}}
& "$root\dotnet\dotnet.exe" "$root\wix-package\tools\net6.0\any\wix.dll" build -arch x64 -d Version=0.1.0 -d 'Stage=C:\OttrinContentQA\payload\Ottrin-0.1.0-windows-x64' -o "$root\current.msi" "$root\installer.wxs"
if($LASTEXITCODE -ne 0){exit $LASTEXITCODE}
(Get-FileHash "$root\current.msi").Hash.ToLowerInvariant()
'''))
    expected=base64.b64decode(result['out-data']).decode().strip().splitlines()[-1];assert len(expected)==64,expected
    download('current.msi',output);assert hashlib.sha256(output.read_bytes()).hexdigest()==expected
    print('Native MSI SHA256',expected,flush=True)
if __name__=='__main__':
    build(sys.argv[1:])
