#!/usr/bin/env python3
"""QA-only adapter: execute the authoritative MSI packager with owned native WiX."""
import base64, hashlib, os, pathlib, runpy, subprocess, sys, time
import guest_exec
domain=os.environ["OTTRIN_MSI_QA_DOMAIN"]
assert domain == "ottrin-index-qa", "Native compiler is restricted to the disposable overlay"
guest_exec.DOMAIN=domain
from guest_exec import qga
args=sys.argv[1:]
assert len(args)==10 and args[:3]==['build','-arch','x64'] and args[3]=='-d' and args[5]=='-d' and args[7]=='-o',args
assert args[4]=='Version=0.1.0'
root=pathlib.Path.cwd().resolve()
dist=pathlib.Path(os.environ["OTTRIN_DIST_DIR"]).resolve()
assert pathlib.Path(args[6].removeprefix('Stage='))==dist/'windows-payload/Ottrin-0.1.0-windows-x64'
source=pathlib.Path(args[9]);assert source==root/'packaging/windows/installer.wxs'
output=pathlib.Path(args[8]);assert output.parent.parent==dist and output.parent.name.startswith('.windows-installer.')
for upload, name in [(source,'msi-final-source.wxs'),(source.with_name('index-task-transaction.ps1'),'index-task-transaction.ps1'),(source.with_name('windows-index-identity.ps1'),'windows-index-identity.ps1')]:
    sys.argv=['guest_upload.py',str(upload),name]
    runpy.run_path(str(pathlib.Path(__file__).with_name('guest_upload.py')),run_name='__main__')
script=r'''$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
& C:\OttrinQA-20261011\msi-wix\dotnet\dotnet.exe C:\OttrinQA-20261011\msi-wix\wix\wix.dll build -arch x64 -d Version=0.1.0 -d 'Stage=C:\OttrinQA-20261011\msi-final-payload\Ottrin-0.1.0-windows-x64' -o C:\OttrinQA-20261011\msi-final.msi C:\OttrinQA-20261011\msi-final-source.wxs
if($LASTEXITCODE -ne 0){exit $LASTEXITCODE}
(Get-FileHash C:\OttrinQA-20261011\msi-final.msi).Hash.ToLowerInvariant()
Start-Sleep 1
'''
pid=qga('guest-exec',{'path':r'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe','arg':['-NoProfile','-NonInteractive','-EncodedCommand',base64.b64encode(script.encode('utf-16le')).decode()],'capture-output':True})['pid']
print(f'Native compiler PID {pid}',flush=True)
while True:
    result=qga('guest-exec-status',{'pid':pid})
    if result.get('exited'):break
    time.sleep(.5)
if result.get('exitcode')!=0:
    for k in ('out-data','err-data'):
        if k in result:print(base64.b64decode(result[k]).decode(errors='replace'),file=sys.stderr)
    raise RuntimeError(f'Native build failed: {result.get("exitcode")}')
expected=base64.b64decode(result['out-data']).decode().strip()
assert len(expected)==64,expected
handle=qga('guest-file-open',{'path':r'C:\OttrinQA-20261011\msi-final.msi','mode':'rb'})
try:
    with output.open('wb') as stream:
        while True:
            data=qga('guest-file-read',{'handle':handle,'count':64*1024})
            if data.get('buf-b64'):stream.write(base64.b64decode(data['buf-b64']))
            if data.get('eof'):break
finally:qga('guest-file-close',{'handle':handle})
actual=hashlib.file_digest(output.open('rb'),'sha256').hexdigest()
assert actual==expected,(actual,expected)
print(f'Native MSI SHA256 {actual}',flush=True)
