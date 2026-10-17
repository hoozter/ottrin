from pathlib import Path
import sys,json,subprocess,time
p=Path(__file__).resolve().parent;sys.path.insert(0,str(p.parent/'windows-copy-cleanup-current'));import transport as n
n.GROOT=r'C:\OttrinCopyCleanupQA'
script=r'''$r='C:\OttrinCopyCleanupQA';if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like"$r\*"}).Count){throw 'Owned package process remains'};$profile=(Get-FileHash 'C:\Users\campbell\AppData\Roaming\ottrin\config.json').Hash;$install=(Get-FileHash 'C:\Users\campbell\AppData\Local\Programs\Ottrin\Uninstall.exe').Hash;if($profile-ne'5DE3C3B0125B564052E1546B34395558BF0F70F1F881DABE2F8A3289FA81371D'-or$install-ne'DDD1178F7C7391D9B344B2A9EC25AED1E82188610D28D340823473D6D21F0EC3'){throw 'Original data changed'};$key=Get-Item 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon';if($key.GetValueNames()-contains'DefaultPassword'){throw 'Temporary credential present'};$backup=Get-Content "$r\desktop-winlogon-backup.json" -Raw|ConvertFrom-Json;foreach($property in $backup.PSObject.Properties){if($property.Name-eq'DefaultPassword'){continue};$b=$property.Value;$present=$key.GetValueNames()-contains$property.Name;if($present-ne$b.present){throw 'Winlogon presence changed'};if($present-and(($key.GetValueKind($property.Name).ToString()-ne$b.kind)-or($key.GetValue($property.Name)-ne$b.value))){throw 'Winlogon changed'}};foreach($dir in @('desktop','compiler','temp')){if(Test-Path (Join-Path $r $dir)){throw 'Owned temporary root remains'}};@{status='pass';source_profile_sha256=$profile;source_installation_sha256=$install;owned_temporary_roots_absent=$true;temporary_login_credential_absent=$true;winlogon_unchanged=$true;owned_executable_processes=0}|ConvertTo-Json|Set-Content "$r\TEARDOWN.json";Get-Content "$r\TEARDOWN.json"'''
pins=json.loads((p/'FIXTURES.json').read_text())['sha256'];helper=json.loads((p/'HELPER.json').read_text())['sha256']
for name,digest in {**pins,'office-probe.exe':helper}.items():
 assert name in ['document.docx','sheet.xlsx','slides.pptx','office-probe.exe']
 pre=r"$f='C:\OttrinCopyCleanupQA\NAME';if((Get-FileHash $f).Hash.ToLowerInvariant()-ne'DIGEST'){throw 'Owned fixture changed; retain it'};if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-eq$f}).Count){throw 'Owned helper live'};Remove-Item -LiteralPath $f".replace('NAME',name).replace('DIGEST',digest.lower())
 n.wait(n.start(pre),45)
for name in ['writer-default','calc-default','calc-bitmap','impress-default','impress-bitmap']:
 for suffix in ['.stdout','.stderr','.pdf']:
  path=r'C:\OttrinCopyCleanupQA\results\office-'+name+suffix
  n.wait(n.start("$f='"+path+"';if(Test-Path $f){Remove-Item -LiteralPath $f}"),45)
n.wait(n.start(r"Remove-Item -LiteralPath C:\OttrinCopyCleanupQA\results\OFFICE_PROBE.json,C:\OttrinCopyCleanupQA\results\OFFICE_PROVIDER.json"),45)
pid=n.start(script)
while True:
 try:n.wait(pid,45);break
 except TimeoutError:print('Observing same teardown PID',flush=True)
n.download('TEARDOWN.json',p/'TEARDOWN.json')
disk=subprocess.check_output(['virsh','-c','qemu:///system','domblklist','ottrin-copy-cleanup-qa'],text=True);assert '/var/lib/libvirt/images/ottrin-copy-cleanup-qa.qcow2' in disk
subprocess.run(['virsh','-c','qemu:///system','shutdown','ottrin-copy-cleanup-qa','--mode','agent'],check=True)
end=time.monotonic()+45
while time.monotonic()<end:
 state=subprocess.check_output(['virsh','-c','qemu:///system','domstate','ottrin-copy-cleanup-qa'],text=True).strip()
 if state=='shut off':break
 time.sleep(1)
assert state=='shut off',state
receipt=json.loads((p/'TEARDOWN.json').read_text(encoding='utf-8-sig'));receipt['owned_vm_state']=state;(p/'TEARDOWN.json').write_text(json.dumps(receipt,indent=2)+'\n')
