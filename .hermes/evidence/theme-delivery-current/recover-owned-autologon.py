"""Restore only the disposable fixture's exhausted one-use automatic login."""
from pathlib import Path
import base64,importlib.util,json,secrets,subprocess,time
r=Path.cwd();e=Path(__file__).resolve().parent/'runtime17-reader';o=json.loads((e/'OWNER.json').read_text())
assert subprocess.check_output(['virsh','-c','qemu:///system','domuuid',o['domain']],text=True).strip()==o['uuid']
assert o['login_reboot_requested'] and (e/'OWNED_SIGNIN.json').exists()
s=importlib.util.spec_from_file_location('guest',r/'.hermes/evidence/windows-20261011/guest_exec.py');g=importlib.util.module_from_spec(s);s.loader.exec_module(g);g.DOMAIN=o['domain']
secret=secrets.token_hex(16)+'aA9!'
code=r"$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$o=Get-Content C:\OttrinOwnedMsixRuntime\OWNER.json -Raw|ConvertFrom-Json;$u=Get-LocalUser -SID $o.sid;if($u.Name-ne'OTMSIXe0453ae87db8'-or$u.Description-ne('Owned MSIX QA '+$o.nonce)){throw 'Exact owned disposable account required'};if((Get-CimInstance Win32_ComputerSystem).UserName){throw 'No active user expected before fixture login recovery'};$key='HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon';$v=Get-ItemProperty $key;if($v.AutoAdminLogon-ne'0'-or$v.DefaultPassword){throw 'Expected exhausted one-use automatic login'};$secret='"+secret+r"';Set-LocalUser -SID $o.sid -Password (ConvertTo-SecureString $secret -AsPlainText -Force);foreach($pair in @(@('AutoAdminLogon','1'),@('DefaultUserName',$o.account),@('DefaultDomainName',$env:COMPUTERNAME),@('DefaultPassword',$secret))){Set-ItemProperty $key -Name $pair[0] -Value $pair[1]};Set-ItemProperty $key -Name AutoLogonCount -Type DWord -Value 3;'owned automatic-login fixture prepared; no Ottrin CLI invoked'"
encoded=base64.b64encode(code.encode('utf-16le')).decode();pid=g.qga('guest-exec',{'path':r'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe','arg':['-NoProfile','-NonInteractive','-EncodedCommand',encoded],'capture-output':True})['pid'];end=time.monotonic()+25
while True:
 result=g.qga('guest-exec-status',{'pid':pid})
 if result.get('exited'):break
 assert time.monotonic()<end,'Owned fixture preparation remains live; do not duplicate'
 time.sleep(.2)
assert result.get('exitcode')==0,'Fixture login preparation failed'
del secret,code,encoded,result
(e/'AUTOLOGON_RECOVERY.json').write_text(json.dumps({'status':'Owned fixture automatic login restored before additional real reboot','domain_uuid':o['uuid'],'source_fix':'runtime.ps1 Prepare explicitly sets AutoLogonCount=3 instead of inheriting the stock image one-use count','scope':'Only generated disposable account credential and its preexisting automatic-login setup; no Ottrin CLI invoked; no security policy changes; secret retained only in owned guest until teardown'},indent=2)+'\n')
g.qga('guest-shutdown',{'mode':'reboot'})
print('Owned fixture prepared and real reboot requested')
