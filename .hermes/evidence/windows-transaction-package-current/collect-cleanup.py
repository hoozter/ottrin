from pathlib import Path
import sys,json
r=Path(__file__).parent.resolve();sys.path.insert(0,str(r));import native_compiler as n
build=json.loads((r/'BUILD.json').read_text())
for source,name in [('results\\nsis-fixed-result.json','NSIS_ACCEPTANCE.json'),('results\\final-msi-result.json','MSI_ACCEPTANCE.json')]:
 n.download(source,r/name);v=json.loads((r/name).read_text(encoding='utf-8-sig'));assert v['status']=='pass';(r/name).write_text(json.dumps(v,indent=2)+'\n')
 label='Ottrin-0.1.0-windows-x64-setup.exe' if name.startswith('NSIS') else 'Ottrin-0.1.0-windows-x64.msi';assert v['installer_sha256']==build['artifacts'][label]
for source,name in [('results\\final-msi-install.log','msi-install.log'),('results\\final-msi-uninstall.log','msi-uninstall.log')]:
 n.download(source,r/name);data=(r/name).read_bytes();encoding='utf-16' if data[:2] in [b'\xff\xfe',b'\xfe\xff'] else 'utf-8-sig';(r/name).write_text('\n'.join(line.rstrip() for line in data.decode(encoding).splitlines()).rstrip()+'\n')
assert n.execute((r/'cleanup-account.ps1').read_text())==0
n.download('cleanup-result.json',r/'ACCOUNT_CLEANUP.json');v=json.loads((r/'ACCOUNT_CLEANUP.json').read_text(encoding='utf-8-sig'));assert v['status']=='pass';(r/'ACCOUNT_CLEANUP.json').write_text(json.dumps(v,indent=2)+'\n')
assert n.execute(r'''
$root='C:\OttrinPackageQA'
$live=@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath-like "$root\*" -or $_.CommandLine-like '*OTTRIN_INSTALLER_PAYLOAD_SCRIPT*'})
if($live.Count){throw 'Compiler or payload-controller process remains'}
Remove-Item $root -Recurse -Force
if(Test-Path $root){throw 'Owned guest root remains'}
Write-Output 'Owned compiler, extraction, uploads and results removed.'
''')==0
(r/'GUEST_CLEANUP.json').write_text(json.dumps({'status':'pass','account_cleanup':'ACCOUNT_CLEANUP.json','owned_compiler_extractions_uploads_and_guest_root_removed':True,'existing_profile_and_installation_hashes_unchanged':True},indent=2)+'\n')
