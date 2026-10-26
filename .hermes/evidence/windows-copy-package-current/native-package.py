"""Bounded current package MSI/loader check; guest/root are always retired."""
from pathlib import Path
import json, subprocess, hashlib, time, os, sys, zipfile, tempfile
import native_compiler as n
p=Path(__file__).resolve().parent;r=Path.cwd();domain=n.guest.DOMAIN
stage=r/'.hermes/packaging-tmp/.windows-ci-stage'
started=False;owned=False
state={'status':'starting','domain':domain,'guest_root':n.GROOT}
# Marker is fixed in this invocation's owned compiler helper, never read from an arbitrary existing root.
import re
nonce=re.search(r"nonce='([a-f0-9]+)'",(p/'native_compiler.py').read_text()).group(1)
def save(): (p/'NATIVE_OWNER.json').write_text(json.dumps(state,indent=2)+'\n')
def sha(f):
 with Path(f).open('rb') as s:return hashlib.file_digest(s,'sha256').hexdigest()
try:
 assert subprocess.check_output(['virsh','-c','qemu:///system','domstate',domain],text=True).strip()=='shut off'
 assert not any(x.strip() in ['cargo','rustc'] for x in subprocess.check_output(['ps','-eo','comm='],text=True).splitlines())
 state['definition_sha256']=hashlib.sha256(subprocess.check_output(['virsh','-c','qemu:///system','dumpxml','--inactive',domain])).hexdigest()
 assert state['definition_sha256']=='f7e51a780fd00ea567c6b17d49374ada34e98884ce66858ca6c05b1a98f35626'
 source=json.loads((p/'BUILD_SOURCE.json').read_text())['source_revision']
 assert source==subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()
 payload=stage/'windows-payload/Ottrin-0.1.0-windows-x64'
 manifest={str(f.relative_to(payload)).replace('/','\\'):sha(f) for f in sorted(payload.rglob('*')) if f.is_file()}
 assert len(manifest)==58
 archive=stage/'Ottrin-0.1.0-windows-x64-portable.zip'
 with zipfile.ZipFile(archive) as z:
  names={str(Path(x.filename).relative_to('Ottrin-0.1.0-windows-x64')).replace('/','\\'):hashlib.sha256(z.read(x)).hexdigest() for x in z.infolist() if not x.is_dir()}
 assert names==manifest
 assert f'Git revision: {source}' in (payload/'SOURCE_REVISION.txt').read_text()
 assert manifest['third-party-notices\\BUNDLED_FONTS.md']==sha(r/'crates/ottrin-ui/assets/THIRD_PARTY_NOTICES.md')
 with tempfile.TemporaryDirectory(prefix='nsis-current-',dir=r/'.hermes/packaging-tmp') as temp:
  subprocess.run(['7z','x','-bd','-y','-o'+temp,str(stage/'Ottrin-0.1.0-windows-x64-setup.exe')],check=True,stdout=subprocess.DEVNULL)
  exe=list(Path(temp).rglob('bin/ottrin.exe'));assert len(exe)==1
  nsis=exe[0].parent.parent
  extracted={str(f.relative_to(nsis)).replace('/','\\'):sha(f) for f in nsis.rglob('*') if f.is_file()}
  assert set(extracted)==set(manifest)|{'Uninstall.exe'}
  assert {name:extracted[name] for name in manifest}==manifest
  uninstaller=(nsis/'Uninstall.exe').read_bytes()
  assert uninstaller[:2]==b'MZ' and b'PE\x00\x00' in uninstaller[:1024]
  (p/'NSIS_PAYLOAD.json').write_text(json.dumps({'status':'pass','application_files':len(manifest),'all_application_bytes_match':True,'additional_generated_file':'Uninstall.exe','uninstaller_sha256':extracted['Uninstall.exe'],'scope':'Generated NSIS uninstaller is installer-specific and intentionally absent from portable application payload'},indent=2)+'\n')
 (p/'PAYLOAD.json').write_text(json.dumps(manifest,indent=2)+'\n')
 (p/'PAYLOAD_GUI_MANIFEST.json').write_text(json.dumps({'source_commit':source,'archive_sha256':sha(archive),'entries':{'Ottrin-0.1.0-windows-x64\\'+name:h for name,h in manifest.items()}},indent=2)+'\n')
 save()
 subprocess.run(['virsh','-c','qemu:///system','start',domain],check=True);started=True
 for a in range(100):
  try:n.qga('guest-ping');break
  except RuntimeError:assert a<99;time.sleep(.5)
 # Capture original profile/install state without exposing any credentials.
 result=n.wait(n.start(r'''$r='C:\OttrinCIPackageQA';if(Test-Path $r){throw 'Unrelated or unfinished package root exists'};$profiles=@(Get-CimInstance Win32_UserProfile|Select-Object SID,LocalPath);$files=@{};foreach($f in @('C:\Users\campbell\AppData\Roaming\ottrin\config.json','C:\Users\campbell\AppData\Local\Programs\Ottrin\Uninstall.exe')){$files[$f]=if(Test-Path $f){(Get-FileHash $f).Hash}else{$null}};@{profiles=$profiles;files=$files}|ConvertTo-Json -Depth 5'''))
 import base64
 baseline=json.loads(base64.b64decode(result['out-data']).decode('utf-8-sig'))
 (p/'NATIVE_BASELINE.json').write_text(json.dumps(baseline,indent=2)+'\n')
 # prepare refuses any existing root before atomically recording this helper's marker.
 owned=True;n.prepare();n.upload(p/'NATIVE_BASELINE.json','BASELINE.json');n.upload(p/'PAYLOAD.json','PAYLOAD.json')
 state['phase']='compile';save()
 result=subprocess.run(['bash','packaging/windows/package-msi.sh'],env={**os.environ,'OTTRIN_DIST_DIR':str(stage),'WIX':str(p/'native_compiler.py')},stdout=(p/'msi-build.log').open('w'),stderr=subprocess.STDOUT)
 assert result.returncode==0
 state['phase']='administrative-extraction';save()
 ps=r'''$r='C:\OttrinCIPackageQA';$m=Get-Content "$r\PAYLOAD.json" -Raw|ConvertFrom-Json;foreach($e in $m.PSObject.Properties){if((Get-FileHash (Join-Path "$r\payload\Ottrin-0.1.0-windows-x64" $e.Name)).Hash.ToLowerInvariant()-ne$e.Value){throw 'Portable payload changed'}};$proc=Start-Process msiexec.exe -ArgumentList @('/a',"`"$r\current.msi`"",'/qn',"TARGETDIR=`"$r\admin`"",'/l*v',"`"$r\administrative.log`"") -PassThru;$null=$proc.Handle;try{if(-not$proc.WaitForExit(30000)){$proc.Kill();throw 'Administrative extraction timed out'};if($proc.ExitCode-ne0){throw "Administrative extraction failed $($proc.ExitCode)"}}finally{$proc.Dispose()};$bins=@(Get-ChildItem "$r\admin" -Filter ottrin.exe -File -Recurse);if($bins.Count-ne1){throw 'Ambiguous administrative payload'};$payload=Split-Path (Split-Path $bins[0].FullName);if(@(Get-ChildItem $payload -File -Recurse).Count-ne@($m.PSObject.Properties).Count){throw 'Administrative payload count mismatch'};foreach($e in $m.PSObject.Properties){if((Get-FileHash (Join-Path $payload $e.Name)).Hash.ToLowerInvariant()-ne$e.Value){throw "MSI payload mismatch $($e.Name)"}};@{status='pass';entries_checked=@($m.PSObject.Properties).Count;msi_sha256=(Get-FileHash "$r\current.msi").Hash.ToLowerInvariant();scope='Native administrative extraction; no install/upgrade/rollback acceptance'}|ConvertTo-Json|Set-Content "$r\MSI_PAYLOAD.json"'''
 n.wait(n.start(ps))
 # Reuse the prior scoped native CLI protocol; output paths remain owned.
 sys.path.insert(0,str(p))
 code=(p.parent/'windows-ci-package-current/native-accept.py').read_text();exec(compile(code,str(p/'native-accept.py'),'exec'),{'__file__':str(p/'native-accept.py')})
 artifacts={name:sha(stage/name) for name in ['Ottrin-0.1.0-windows-x64-portable.zip','Ottrin-0.1.0-windows-x64-setup.exe','Ottrin-0.1.0-windows-x64.msi']}
 (p/'HOST_PAYLOAD.json').write_text(json.dumps({'status':'pass','source_revision':source,'files':58,'artifacts':artifacts,'portable_nsis_msi_agree':True,'font_notice_exact':True,'gui_acceptance':'pending'},indent=2)+'\n')
 desktop=n.wait(n.start(r"""$k=[Microsoft.Win32.Registry]::LocalMachine.OpenSubKey('SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon',$false);$passwordPresent='DefaultPassword'-in$k.GetValueNames();$k.Dispose();@{console_user=(Get-CimInstance Win32_ComputerSystem).UserName;winlogon_password_present=$passwordPresent;interactive_processes=@(Get-CimInstance Win32_Process|Where-Object{$_.SessionId-ne0}|Select-Object Name,ExecutablePath,ProcessId,ParentProcessId,SessionId)}|ConvertTo-Json -Depth 5"""))
 (p/'DESKTOP_BASELINE.json').write_text(json.dumps(json.loads(base64.b64decode(desktop['out-data']).decode('utf-8-sig')),indent=2)+'\n')
 state['status']='passed named package scopes; teardown pending'
except BaseException as error:
 state['status']='failed';state['error']=repr(error);raise
finally:
 try:
  if owned:
   ps=r'''$r='C:\OttrinCIPackageQA';if((Get-Content "$r\OWNER.json" -Raw|ConvertFrom-Json).nonce-ne'NONCE'){throw 'Refuse unrelated package root'};if(@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath-and$_.ExecutablePath.StartsWith($r+'\',[StringComparison]::OrdinalIgnoreCase)}).Count){throw 'Owned compiler or CLI process remains'};$b=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('BASELINE_B64'))|ConvertFrom-Json;foreach($e in $b.files.PSObject.Properties){if($e.Value){if((Get-FileHash $e.Name).Hash-ne$e.Value){throw 'Original file changed'}}elseif(Test-Path $e.Name){throw 'Previously absent original file appeared'}};$before=@($b.profiles|ForEach-Object{$_.SID+'|'+$_.LocalPath}|Sort-Object);$after=@(Get-CimInstance Win32_UserProfile|ForEach-Object{$_.SID+'|'+$_.LocalPath}|Sort-Object);if(Compare-Object $before $after){throw 'Original profiles changed'};Remove-Item $r -Recurse -Force;if(Test-Path $r){throw 'Owned package root remains'};@{status='pass';root_removed=$true;owned_processes=0;original_files_and_profiles_preserved=$true}|ConvertTo-Json'''.replace('NONCE',nonce).replace('BASELINE_B64',base64.b64encode(json.dumps(baseline).encode()).decode())
   result=n.wait(n.start(ps));(p/'TEARDOWN_GUEST.json').write_bytes(base64.b64decode(result['out-data']));state['root_removed']=True
 except BaseException as error:state['status']='failed teardown';state['teardown_error']=repr(error);raise
 finally:
  if started:
   try:n.qga('guest-shutdown',{'mode':'powerdown'})
   except RuntimeError:state['shutdown_transport_disconnected']=True
   for a in range(90):
    if subprocess.check_output(['virsh','-c','qemu:///system','domstate',domain],text=True).strip()=='shut off':break
    time.sleep(.5)
   else:raise RuntimeError('Guest did not finish shutdown')
   state['guest_off']=True
   assert state['definition_sha256']==hashlib.sha256(subprocess.check_output(['virsh','-c','qemu:///system','dumpxml','--inactive',domain])).hexdigest()
  if state['status']=='passed named package scopes; teardown pending' and state.get('root_removed') and state.get('guest_off'):state['status']='pass'
  save()
