"""Native sign-in to an observed, exact disposable account; never retain its credential."""
from pathlib import Path
import argparse,base64,hashlib,importlib.util,json,os,re,secrets,subprocess,time
r=Path.cwd();parser=argparse.ArgumentParser();parser.add_argument('--case',type=Path,required=True);e=parser.parse_args().case.resolve();assert e.parent==Path(__file__).resolve().parent
owner=json.loads((e/'OWNER.json').read_text());assert not owner.get('cleanup');assert not (e/'NATIVE_SIGN_IN.json').exists(),'An owned sign-in was already submitted or uncertain; observe it instead of repeating'
assert subprocess.check_output(['virsh','-c','qemu:///system','domuuid',owner['domain']],text=True).strip()==owner['uuid']
prepared=json.loads(json.loads((e/'PREPARATION.json').read_text())['stdout']);account=prepared['account'];sid=prepared['sid'];assert re.fullmatch('OTMSIX[a-f0-9]{12}',account)
frame=e/'sign-in-ready.png';text=subprocess.check_output(['tesseract',str(frame),'stdout','--psm','11'],stderr=subprocess.DEVNULL,text=True).casefold();assert account.casefold() in text,'Observed account does not match';review=json.loads((e/'SIGN_IN_VISUAL_REVIEW.json').read_text());assert review=={'account':account,'frame_sha256':hashlib.sha256(frame.read_bytes()).hexdigest(),'focused_password_field':True},'Review the exact native password field';assert time.time()-frame.stat().st_mtime<30,'Native sign-in observation expired'
meta=owner['private_console'];p=Path('/proc')/str(meta['pid']);args=(p/'cmdline').read_bytes().split(b'\0');assert owner['domain'].encode() in args and os.readlink(p/'exe')=='/usr/bin/virt-viewer'
values=dict(v.split(b'=',1) for v in (p/'environ').read_bytes().split(b'\0') if b'=' in v);assert os.fsdecode(values[b'DISPLAY'])==meta['display'] and os.fsdecode(values[b'HOME'])==meta['home']
assert meta['home'].startswith('/var/tmp/.ottrin-picker-qa-')
env={**os.environ,'DISPLAY':meta['display'],'GDK_BACKEND':'x11','EGL_PLATFORM':'x11'};env.pop('XAUTHORITY',None)
windows=subprocess.check_output(['xdotool','search','--onlyvisible','--pid',str(meta['pid'])],env=env,text=True).splitlines();assert len(windows)==1;window=windows[0]
spec=importlib.util.spec_from_file_location('guest',r/'.hermes/evidence/windows-20261011/guest_exec.py');g=importlib.util.module_from_spec(spec);spec.loader.exec_module(g);g.DOMAIN=owner['domain']
receipt={'status':'intent recorded','domain_uuid':owner['uuid'],'account':account,'sid':sid,'observed_frame_sha256':hashlib.sha256(frame.read_bytes()).hexdigest(),'scope':'Reset only the exact disposable account credential and enter it once in the observed native password field; no reboot, package command, startup-task or security-policy change; credential never printed or persisted'}
(e/'NATIVE_SIGN_IN.json').write_text(json.dumps(receipt,indent=2)+'\n')
secret=secrets.token_hex(18)+'aA1!'
try:
 code=r'''$ErrorActionPreference='Stop';$o=Get-Content 'C:\OttrinOwnedMsixRuntime\OWNER.json' -Raw|ConvertFrom-Json;if($o.account-ne'ACCOUNT'-or$o.sid-ne'SID'){throw 'Wrong disposable account'};$u=Get-LocalUser -Name $o.account;if($u.SID.Value-ne$o.sid-or$u.Description-ne('Owned MSIX QA '+$o.nonce)){throw 'Account ownership changed'};$p=ConvertTo-SecureString 'SECRET' -AsPlainText -Force;try{Set-LocalUser -SID $u.SID -Password $p}finally{$p.Dispose()}'''.replace("'ACCOUNT'",repr(account)).replace("'SID'",repr(sid)).replace("'SECRET'",repr(secret))
 encoded=base64.b64encode(code.encode('utf-16le')).decode();pid=g.qga('guest-exec',{'path':r'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe','arg':['-NoProfile','-NonInteractive','-EncodedCommand',encoded],'capture-output':True})['pid'];receipt['credential_process_pid']=pid;(e/'NATIVE_SIGN_IN.json').write_text(json.dumps(receipt,indent=2)+'\n');deadline=time.monotonic()+30
 while True:
  result=g.qga('guest-exec-status',{'pid':pid})
  if result.get('exited'):break
  assert time.monotonic()<deadline,'Owned credential observer timeout; do not repeat mutation';time.sleep(.3)
 receipt['credential_reset_exit']=result.get('exitcode');assert result.get('exitcode')==0,'Owned credential reset failed; credential and command output deliberately omitted'
 subprocess.run(['xdotool','windowactivate','--sync',window,'key','--clearmodifiers','ctrl+a'],env=env,check=True,timeout=10)
 subprocess.run(['xdotool','type','--clearmodifiers','--delay','30','--file','-'],input=secret,text=True,env=env,check=True,timeout=10)
 subprocess.run(['xdotool','key','--clearmodifiers','Return'],env=env,check=True,timeout=10)
 receipt['status']='single native sign-in submitted; actual login/startup still requires native actor proof'
finally:
 secret=None;code=None;encoded=None
 (e/'NATIVE_SIGN_IN.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps(receipt))
