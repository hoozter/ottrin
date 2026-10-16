from pathlib import Path
import subprocess,json,sys
p=Path(__file__).resolve().parent
artifacts=['Ottrin-0.1.0-x86_64.AppImage','Ottrin-0.1.0-linux-x86_64-portable.tar.gz','ottrin_0.1.0_amd64.deb','ottrin-0.1.0-1.x86_64.rpm']
r={'status':'running','artifacts':{}}
try:
 for artifact in artifacts:
  with (p/(artifact+'.qa.log')).open('w') as out:code=subprocess.run([sys.executable,str(p/'linux-content-qa.py'),str(Path('.hermes/packaging-tmp/eps-refresh')/artifact)],stdout=out,stderr=subprocess.STDOUT).returncode
  r['artifacts'][artifact]={'exit':code};assert code==0,artifact+' failed; inspect owned log'
 r['status']='pass'
finally:(p/'LINUX_PACKAGES_RUN.json').write_text(json.dumps(r,indent=2)+'\n')
print('All four exact Linux package CLI checks passed')
