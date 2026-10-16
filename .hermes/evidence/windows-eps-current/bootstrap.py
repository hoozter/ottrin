from pathlib import Path
import sys,time,subprocess
p=Path(__file__).resolve().parent;sys.path.insert(0,str(p));import transport as n
assert subprocess.check_output(['virsh','-c','qemu:///system','domstate',n.guest.DOMAIN],text=True).strip()=='running'
deadline=time.monotonic()+45
while True:
 try:n.qga('guest-ping',{});break
 except Exception:
  if time.monotonic()>deadline:raise TimeoutError('Owned VM live; guest agent not ready, poll same domain')
  time.sleep(2)
assert n.execute(r'''
$root='C:\OttrinContentQA';if(Test-Path $root){throw 'Owned root already exists'}
New-Item -ItemType Directory $root|Out-Null
@{preference_hash=(Get-FileHash C:\Users\campbell\AppData\Roaming\ottrin\config.json).Hash;prior_uninstaller_hash=(Get-FileHash C:\Users\campbell\AppData\Local\Programs\Ottrin\Uninstall.exe).Hash}|ConvertTo-Json|Set-Content "$root\baseline.json"
New-Item 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\OOBE' -Force|Out-Null
Set-ItemProperty 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\OOBE' DisablePrivacyExperience 1 -Type DWord
Set-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System' EnableFirstLogonAnimation 0 -Type DWord
'Owned root and nonsecret baseline prepared'
''')==0
