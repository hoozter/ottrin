"""Recover only the nonce-bound disposable MSIX account's consumed auto-login."""
from pathlib import Path
import base64,importlib.util,json,subprocess,time
r=Path.cwd();e=Path(__file__).resolve().parent;case=e/'upgrade-enabled';owner=json.loads((case/'OWNER.json').read_text());assert owner['status']=='observing actual Windows login startup before any Ottrin CLI'
assert subprocess.check_output(['virsh','-c','qemu:///system','domuuid',owner['domain']],text=True).strip()==owner['uuid']
s=importlib.util.spec_from_file_location('guest',r/'.hermes/evidence/windows-20261011/guest_exec.py');g=importlib.util.module_from_spec(s);s.loader.exec_module(g);g.DOMAIN=owner['domain']
record={'status':'recovery starting','domain_uuid':owner['uuid'],'scope':'Only fixture-owned temporary MSIX account credential and automatic login; no Ottrin CLI, startup setting, signing policy, original VM or user-data changes'}
def save():(case/'LOGIN_RECOVERY.json').write_text(json.dumps(record,indent=2)+'\n')
def ps(code,seconds=35):
 encoded=base64.b64encode(code.encode('utf-16-le')).decode();pid=g.qga('guest-exec',{'path':r'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe','arg':['-NoProfile','-NonInteractive','-EncodedCommand',encoded],'capture-output':True})['pid'];deadline=time.monotonic()+seconds
 while True:
  result=g.qga('guest-exec-status',{'pid':pid})
  if result.get('exited'):break
  assert time.monotonic()<deadline;time.sleep(.3)
 assert result['exitcode']==0,base64.b64decode(result.get('err-data','')).decode(errors='replace')
 return json.loads(base64.b64decode(result.get('out-data','')).decode('utf-8-sig'))
save()
code=r'''$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$root='C:\OttrinOwnedMsixRuntime';$o=Get-Content ($root+'\OWNER.json') -Raw|ConvertFrom-Json;$user=Get-LocalUser -SID $o.sid;if($user.Name-ne$o.account-or$user.Description-ne('Owned MSIX QA '+$o.nonce)){throw 'Not exact fixture-created account'};if((Get-CimInstance Win32_ComputerSystem).UserName){throw 'Console already has a user; no credential change needed'};$key='HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon';$old=Get-ItemProperty $key;if($old.DefaultUserName-ne$o.account-or$old.AutoAdminLogon-ne'0'-or([string]$old.DefaultPassword).Length){throw 'Not consumed fixture auto-login'};if(@(Get-Process ottrin,ottrin-indexd -ErrorAction SilentlyContinue).Count){throw 'Unexpected product process before login'};$secret=[Guid]::NewGuid().ToString('N')+'aA1!';$password=ConvertTo-SecureString $secret -AsPlainText -Force;try{Set-LocalUser -SID $o.sid -Password $password;foreach($pair in @(@('AutoAdminLogon','1'),@('DefaultUserName',$o.account),@('DefaultDomainName',$env:COMPUTERNAME),@('DefaultPassword',$secret))){Set-ItemProperty $key -Name $pair[0] -Value $pair[1]};New-ItemProperty $key -Name AutoLogonCount -PropertyType DWord -Value 1 -Force|Out-Null;$before=(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().Ticks;& shutdown.exe /r /t 15;if($LASTEXITCODE){throw 'Normal reboot request failed'};@{sid=$o.sid;nonce=$o.nonce;before_boot_ticks=$before;previous_auto_admin_logon=$old.AutoAdminLogon;previous_password_present=$false;fixture_account_password_reset=$true;reboot_command='shutdown.exe /r /t 15';no_ottrin_cli=$true}|ConvertTo-Json -Compress}finally{$secret=$null;$password.Dispose()}'''
record.update(ps(code));assert record['before_boot_ticks']==json.loads((case/'REBOOT_2.json').read_text())['after_boot_ticks'];record['status']='normal recovery reboot requested';save();deadline=time.monotonic()+180
while time.monotonic()<deadline:
 time.sleep(3)
 try:
  value=ps("$ErrorActionPreference='Stop';@{ticks=(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().Ticks;console_user=(Get-CimInstance Win32_ComputerSystem).UserName}|ConvertTo-Json -Compress",15)
  if value['ticks']!=record['before_boot_ticks']:
   record.update(status='normal recovery reboot verified; original login actor remains authoritative',after_boot_ticks=value['ticks'],observed_console_user=value['console_user']);save();break
 except (RuntimeError,AssertionError,KeyError,TimeoutError,json.JSONDecodeError):pass
else:raise TimeoutError('Owned recovery reboot not observed; do not restart the test')
