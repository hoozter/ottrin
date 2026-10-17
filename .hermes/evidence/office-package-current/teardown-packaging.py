from pathlib import Path
import sys,json,subprocess,time
p=Path(__file__).resolve().parent;sys.path.insert(0,str(p.parent/'windows-copy-cleanup-current'));import transport as n
n.GROOT=r'C:\OttrinCopyCleanupQA'
script=r'''$r='C:\OttrinCopyCleanupQA';if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like"$r\desktop\*"-or$_.ExecutablePath-like"$r\compiler\*"}).Count){throw 'Owned package process remains'};$profile=(Get-FileHash 'C:\Users\campbell\AppData\Roaming\ottrin\config.json').Hash;$install=(Get-FileHash 'C:\Users\campbell\AppData\Local\Programs\Ottrin\Uninstall.exe').Hash;if($profile-ne'5DE3C3B0125B564052E1546B34395558BF0F70F1F881DABE2F8A3289FA81371D'-or$install-ne'DDD1178F7C7391D9B344B2A9EC25AED1E82188610D28D340823473D6D21F0EC3'){throw 'Original data changed'};$key=Get-Item 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon';if($key.GetValueNames()-contains'DefaultPassword'){throw 'Temporary credential present'};$backup=Get-Content "$r\desktop-winlogon-backup.json" -Raw|ConvertFrom-Json;foreach($property in $backup.PSObject.Properties){if($property.Name-eq'DefaultPassword'){continue};$b=$property.Value;$present=$key.GetValueNames()-contains$property.Name;if($present-ne$b.present){throw 'Winlogon presence changed'};if($present-and(($key.GetValueKind($property.Name).ToString()-ne$b.kind)-or($key.GetValue($property.Name)-ne$b.value))){throw 'Winlogon changed'}};$task=Get-ScheduledTask -TaskName 'Ottrin Office Interactive QA' -ErrorAction Stop;if($task.State-eq'Running'){throw 'Office QA task still running'};Unregister-ScheduledTask -TaskName 'Ottrin Office Interactive QA' -Confirm:$false;foreach($pin in @(@('mergedlo.dll','017D0C21B6E45E54C0194B58017853D8C653059CC643643C3EB831A4D309331F'),@('vclplug_winlo.dll','BBA69CA1047FB46866F8982E90C995B559E2C95FA36258CE8ABFE339BFD53715'))){if((Get-FileHash (Join-Path 'C:\Program Files\LibreOffice\program' $pin[0])).Hash-ne$pin[1]){throw 'LibreOffice provider changed'}};foreach($dir in @('desktop','compiler')){Remove-Item -LiteralPath (Join-Path $r $dir) -Recurse -Force};@{status='pass';source_profile_sha256=$profile;source_installation_sha256=$install;compiler_and_payload_extractions_removed=$true;temporary_login_credential_absent=$true;winlogon_unchanged=$true;owned_executable_processes=0}|ConvertTo-Json|Set-Content "$r\PACKAGE_TEARDOWN.json";Get-Content "$r\PACKAGE_TEARDOWN.json"'''
pid=n.start(script)
while True:
 try:n.wait(pid,45);break
 except TimeoutError:print('Observing same teardown PID',flush=True)
n.download('PACKAGE_TEARDOWN.json',p/'PACKAGE_TEARDOWN.json')
disk=subprocess.check_output(['virsh','-c','qemu:///system','domblklist','ottrin-copy-cleanup-qa'],text=True);assert '/var/lib/libvirt/images/ottrin-copy-cleanup-qa.qcow2' in disk
subprocess.run(['virsh','-c','qemu:///system','shutdown','ottrin-copy-cleanup-qa','--mode','agent'],check=True)
end=time.monotonic()+45
while time.monotonic()<end:
 state=subprocess.check_output(['virsh','-c','qemu:///system','domstate','ottrin-copy-cleanup-qa'],text=True).strip()
 if state=='shut off':break
 time.sleep(1)
assert state=='shut off',state
receipt=json.loads((p/'PACKAGE_TEARDOWN.json').read_text(encoding='utf-8-sig'));receipt['owned_vm_state']=state;(p/'PACKAGE_TEARDOWN.json').write_text(json.dumps(receipt,indent=2)+'\n')
