#!/usr/bin/env python3
"""Owned-QA native compiler transport; never writes outside the scoped guest root."""
import base64,hashlib,json,os,pathlib,sys,time,subprocess
ROOT=pathlib.Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT/'.hermes/evidence/windows-20261011'))
import guest_exec as guest
guest.DOMAIN='ottrin-init-windows-copy-inheritance-qa'
assert guest.DOMAIN=='ottrin-init-windows-copy-inheritance-qa'
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
