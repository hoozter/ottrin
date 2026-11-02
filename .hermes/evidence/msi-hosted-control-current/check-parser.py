"""Read-only native PowerShell parsing; no policy or filesystem changes in the guest."""
from pathlib import Path
import sys,base64,json,hashlib,time,gzip
r=Path.cwd();sys.path.insert(0,str(r/'.hermes/evidence/windows-office-contract-current'));import transport as n
out=Path(__file__).resolve().parent;results=[]
deadline=time.monotonic()+45
while True:
 try:n.qga('guest-ping');break
 except RuntimeError:
  assert time.monotonic()<deadline,'Existing owned VM guest agent is not ready'
  time.sleep(.5)
for f in sorted((r/'.hermes/packaging-tmp/.msi-hosted-control/scripts/ci').glob('msi-rollback-control*.ps1')):
 raw=f.read_bytes();encoded=base64.b64encode(gzip.compress(raw,mtime=0)).decode();script="$ErrorActionPreference='Stop';$bytes=[Convert]::FromBase64String('"+encoded+"');$memory=[IO.MemoryStream]::new($bytes);$gzip=[IO.Compression.GZipStream]::new($memory,[IO.Compression.CompressionMode]::Decompress);$reader=[IO.StreamReader]::new($gzip,[Text.Encoding]::UTF8);try{$s=$reader.ReadToEnd()}finally{$reader.Dispose();$gzip.Dispose();$memory.Dispose()};$tokens=$null;$errors=$null;$null=[Management.Automation.Language.Parser]::ParseInput($s,[ref]$tokens,[ref]$errors);@{errors=@($errors|ForEach-Object {@{message=$_.Message;line=$_.Extent.StartLineNumber}});count=$errors.Count}|ConvertTo-Json -Depth 6"
 payload=base64.b64encode(script.encode('utf-16le')).decode();assert len(payload)<30000
 pid=n.qga('guest-exec',{'path':r'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe','arg':['-NoProfile','-NonInteractive','-EncodedCommand',payload],'capture-output':True})['pid']
 deadline=time.monotonic()+30
 while True:
  result=n.qga('guest-exec-status',{'pid':pid})
  if result.get('exited'):break
  assert time.monotonic()<deadline,'Read-only parser did not exit; retain owned PID for inspection';time.sleep(.1)
 assert result['exitcode']==0,result
 parsed=json.loads(base64.b64decode(result['out-data']).decode('utf-8-sig'));results.append({'file':f.name,'sha256':hashlib.sha256(raw).hexdigest(),'native_pid':pid,'result':parsed})
(out/'PARSER.json').write_text(json.dumps(results,indent=2)+'\n');assert all(x['result']['count']==0 for x in results),results
print('Native PowerShell parse passes for both owned control scripts; no guest state changed.')
