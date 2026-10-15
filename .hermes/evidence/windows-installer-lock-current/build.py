from pathlib import Path
import subprocess,json,hashlib,shutil
root=Path.cwd();ev=root/'.hermes/evidence/windows-installer-lock-current';tmp=root/'.hermes/packaging-tmp/installer-lock-current';stage=tmp/'stage'
(stage/'bin').mkdir(parents=True,exist_ok=True)
subprocess.run(['x86_64-w64-mingw32-gcc','-municode',str(ev/'mock-payload.c'),'-o',str(stage/'bin/ottrin.exe')],check=True)
shutil.copyfile(stage/'bin/ottrin.exe',stage/'bin/ottrin-indexd.exe')
manifest={'scope':'QA-only full installer scripts with isolated registration/shortcut/data names and mock executables; actual shared mutex retained','source_sha256':hashlib.file_digest((root/'packaging/windows/installer.nsi').open('rb'),'sha256').hexdigest(),'artifacts':{}}
for label,source in [('baseline',subprocess.check_output(['git','show','c6064a03a672be92af157ccbc6b337af6d90f08c:packaging/windows/installer.nsi'],text=True)),('fixed',(root/'packaging/windows/installer.nsi').read_text())]:
 source=source.replace('Software\\Ottrin"','Software\\OttrinInstallerLockQA"').replace('Uninstall\\Ottrin"','Uninstall\\OttrinInstallerLockQA"').replace('$SMPROGRAMS\\Ottrin','$SMPROGRAMS\\OttrinInstallerLockQA').replace('$LOCALAPPDATA\\ottrin"','$LOCALAPPDATA\\ottrin-installer-lock-qa"').replace('$LOCALAPPDATA\\ottrin\\installer-index-transaction.json','$LOCALAPPDATA\\ottrin-installer-lock-qa\\installer-index-transaction.json')
 # Keep this exact scoped script beside the receipts. No substituted production
 # package or installer is published by this behavior fixture.
 script=ev/(label+'.nsi');script.write_text(source)
 out=tmp/(label+'.exe')
 with (ev/(label+'-build.log')).open('w') as log:subprocess.run(['makensis','-V2','-DSTAGE='+str(stage),'-DOUTPUT='+str(out),str(script)],check=True,stdout=log,stderr=subprocess.STDOUT)
 manifest['artifacts'][label]={'sha256':hashlib.file_digest(out.open('rb'),'sha256').hexdigest(),'script_sha256':hashlib.file_digest(script.open('rb'),'sha256').hexdigest()}
(ev/'BUILD.json').write_text(json.dumps(manifest,indent=2)+'\n')
