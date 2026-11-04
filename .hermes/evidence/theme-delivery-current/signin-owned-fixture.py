"""Normal credential entry for the observed disposable MSIX QA account only."""
from pathlib import Path
import base64,hashlib,importlib.util,json,os,secrets,subprocess,time
r=Path.cwd();e=Path(__file__).resolve().parent/'runtime17-reader';o=json.loads((e/'OWNER.json').read_text())
assert subprocess.check_output(['virsh','-c','qemu:///system','domuuid',o['domain']],text=True).strip()==o['uuid']
assert o['login_reboot_requested'] and (e/'owned-signin-click.png').is_file()
meta=o['private_console'];p=Path('/proc')/str(meta['pid']);assert o['domain'].encode() in (p/'cmdline').read_bytes().split(b'\0') and os.readlink(p/'exe')=='/usr/bin/virt-viewer'
values=dict(v.split(b'=',1) for v in (p/'environ').read_bytes().split(b'\0') if b'=' in v);assert os.fsdecode(values[b'DISPLAY'])==meta['display'] and os.fsdecode(values[b'HOME'])==meta['home']
env={**os.environ,'DISPLAY':meta['display']};env.pop('XAUTHORITY',None)
s=importlib.util.spec_from_file_location('guest',r/'.hermes/evidence/windows-20261011/guest_exec.py');g=importlib.util.module_from_spec(s);s.loader.exec_module(g);g.DOMAIN=o['domain']
secret=secrets.token_hex(16)+'aA1!'
code=r"$ErrorActionPreference='Stop';$o=Get-Content C:\OttrinOwnedMsixRuntime\OWNER.json -Raw|ConvertFrom-Json;$u=Get-LocalUser -SID $o.sid;if($u.Name-ne'OTMSIXe0453ae87db8'-or$u.Name-ne$o.account-or$u.Description-ne('Owned MSIX QA '+$o.nonce)){throw 'Exact owned disposable account required'};Set-LocalUser -SID $o.sid -Password (ConvertTo-SecureString '"+secret+"' -AsPlainText -Force);'owned account credential prepared'"
encoded=base64.b64encode(code.encode('utf-16le')).decode();pid=g.qga('guest-exec',{'path':r'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe','arg':['-NoProfile','-NonInteractive','-EncodedCommand',encoded],'capture-output':True})['pid']
end=time.monotonic()+25
while True:
 result=g.qga('guest-exec-status',{'pid':pid})
 if result.get('exited'):break
 assert time.monotonic()<end,'Owned credential command still live; do not duplicate'
 time.sleep(.2)
assert result.get('exitcode')==0,'Owned credential preparation failed; no keyboard input sent'
# The screenshot has the exact owned account selected and its password field focused.
# Feed the disposable password on stdin, never argv, a file, log or tool output.
subprocess.run(['xdotool','type','--clearmodifiers','--file','-'],input=secret,text=True,env=env,check=True,timeout=10)
subprocess.run(['xdotool','key','Return'],env=env,check=True,timeout=10)
del secret,code,encoded,result
(e/'OWNED_SIGNIN.json').write_text(json.dumps({'status':'normal credential submitted to observed owned account','domain_uuid':o['uuid'],'account':'OTMSIXe0453ae87db8','observed_frame_sha256':hashlib.sha256((e/'owned-signin-click.png').read_bytes()).hexdigest(),'scope':'Disposable account only; temporary credential reset and standard sign-in; no Ottrin CLI launch, no security policy changes, no host password retained'},indent=2)+'\n')
print('Owned sign-in submitted without retaining its temporary password')
