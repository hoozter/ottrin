from pathlib import Path
import sys,json,hashlib
r=Path(__file__).parent.resolve();sys.path.insert(0,str(r));import transport as n
root=r.parents[2];stage=root/'.hermes/packaging-tmp/windows-current/windows-payload/Ottrin-0.1.0-windows-x64';tmp=root/'.hermes/packaging-tmp/full-recovery-current'
manifest=json.loads((root/'.hermes/evidence/windows-transaction-package-current/FINAL_ARCHIVE_CONTENTS.json').read_text())['payload_sha256'];(r/'payload.json').write_text(json.dumps(manifest,indent=2)+'\n')
files=[(root/'dist/Ottrin-0.1.0-windows-x64-setup.exe','baseline.exe'),(root/'.hermes/packaging-tmp/windows-current/Ottrin-0.1.0-windows-x64-setup.exe','current.exe'),(tmp/'pause.exe','pause.exe'),(tmp/'rollback.exe','rollback.exe'),(stage/'bin/ottrin-indexd.exe','controller.exe'),(r/'payload-transaction.qa.ps1','payload-transaction.qa.ps1'),(r/'payload.json','payload.json'),(root/'.hermes/evidence/address-selection-current/VALID_CONFIG_TEMPLATE.json','config-template.json'),(r/'actor.ps1','actor.ps1')]
assert n.execute(r'''
$explorers=@(Get-CimInstance Win32_Process -Filter "Name='explorer.exe'"|Where-Object {(Invoke-CimMethod $_ -MethodName GetOwner).User-eq'OttrinFullRecoveryQA'})
if($explorers.Count-ne1){throw 'Owned interactive session is not ready'}
''')==0
b=json.loads((r/'BUILD.json').read_text());b['guest_inputs_sha256']={name:hashlib.sha256(p.read_bytes()).hexdigest() for p,name in files}
for p,name in files:n.upload(p,name)
(r/'BUILD.json').write_text(json.dumps(b,indent=2)+'\n')
assert n.execute(r'''
$root='C:\OttrinFullRecoveryQA'
$a=New-ScheduledTaskAction -Execute powershell.exe -Argument '-NoProfile -NonInteractive -ExecutionPolicy Bypass -File C:\OttrinFullRecoveryQA\actor.ps1'
$p=New-ScheduledTaskPrincipal -UserId "$env:COMPUTERNAME\OttrinFullRecoveryQA" -LogonType Interactive -RunLevel Limited
$s=New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([TimeSpan]::Zero)
Register-ScheduledTask 'Ottrin Full Recovery QA' -Action $a -Principal $p -Settings $s|Out-Null
Start-ScheduledTask 'Ottrin Full Recovery QA'
Write-Output ((Get-ScheduledTask 'Ottrin Full Recovery QA').State)
''')==0
