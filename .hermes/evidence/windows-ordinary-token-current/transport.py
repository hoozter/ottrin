from pathlib import Path,PureWindowsPath
import sys,base64,subprocess,json,time
ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT/'.hermes/evidence/windows-20261011'))
import guest_exec as guest
guest.DOMAIN='ottrin-copy-cleanup-qa'
qga=guest.qga
GROOT=r'C:\OttrinOrdinaryQA'
def remote(name):
 p=PureWindowsPath(name);assert not p.is_absolute() and '..' not in p.parts
 return str(PureWindowsPath(GROOT)/p)
def upload(source,name):
 h=qga('guest-file-open',{'path':remote(name),'mode':'wb'})
 try:
  with Path(source).open('rb') as f:
   while b:=f.read(65536):
    result=qga('guest-file-write',{'handle':h,'buf-b64':base64.b64encode(b).decode()});assert result['count']==len(b)
  qga('guest-file-flush',{'handle':h})
 finally:qga('guest-file-close',{'handle':h})
def download(name,path):
 h=qga('guest-file-open',{'path':remote(name),'mode':'rb'})
 try:
  with Path(path).open('wb') as f:
   while True:
    result=qga('guest-file-read',{'handle':h,'count':65536})
    if result.get('buf-b64'):f.write(base64.b64decode(result['buf-b64']))
    if result.get('eof'):break
 finally:qga('guest-file-close',{'handle':h})
def start(script):
 return qga('guest-exec',{'path':r'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe','arg':['-NoProfile','-NonInteractive','-EncodedCommand',base64.b64encode(("$ProgressPreference='SilentlyContinue'\n"+script).encode('utf-16le')).decode()],'capture-output':True})['pid']
def poll(pid):
 r=qga('guest-exec-status',{'pid':pid})
 if r.get('exited'):
  return {'exit':r.get('exitcode'),**{k:base64.b64decode(r.get(k,b'')).decode(errors='replace') for k in ['out-data','err-data']}}
def execute(script):
 pid=start(script);end=time.monotonic()+45
 while time.monotonic()<end:
  r=poll(pid)
  if r is not None:
   assert r['exit']==0,r
   return r
  time.sleep(.25)
 raise TimeoutError('Guest PID remains live; poll same PID '+str(pid))
