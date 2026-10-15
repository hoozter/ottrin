from pathlib import Path
import subprocess,shutil,hashlib,json
root=Path.cwd();ev=root/'.hermes/evidence/windows-commit-current';temp=root/'.hermes/packaging-tmp/commit-current';receipts={}
for label,version in [('baseline','old'),('fixed','new'),('staging-fault','new')]:
 stage=temp/label/'stage';(stage/'bin').mkdir(parents=True,exist_ok=True)
 subprocess.run(['x86_64-w64-mingw32-gcc','-municode','-DPAYLOAD_VERSION=L"'+version+'"',str(ev/'mock.c'),'-o',str(stage/'bin/ottrin.exe')],check=True)
 shutil.copyfile(stage/'bin/ottrin.exe',stage/'bin/ottrin-indexd.exe');(stage/'version.txt').write_text(version)
 out=temp/(label+'.exe')
 with (ev/(label+'-build.log')).open('w') as log:
  subprocess.run(['makensis','-V2','-DSTAGE='+str(stage),'-DVERSION='+('0.1.0' if label=='baseline' else '0.2.0'),'-DPAYLOAD_TRANSACTION_SCRIPT='+str(ev/'payload-transaction.qa.ps1'),'-DOUTPUT='+str(out),str(ev/(label+'.nsi'))],check=True,stdout=log,stderr=subprocess.STDOUT)
 receipts[label]=hashlib.file_digest(out.open('rb'),'sha256').hexdigest()
(ev/'BUILD.json').write_text(json.dumps(receipts,indent=2)+'\n')
