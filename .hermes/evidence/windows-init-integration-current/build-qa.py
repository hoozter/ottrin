from pathlib import Path
import subprocess,json,hashlib
r=Path.cwd();ev=r/'.hermes/evidence/windows-init-integration-current';tmp=r/'.hermes/packaging-tmp/init-integration-qa';tmp.mkdir();stage=r/'.hermes/packaging-tmp/windows-next/windows-payload/Ottrin-0.1.0-windows-x64'
source=(r/'packaging/windows/payload-transaction.ps1').read_text()
for point,needle in [('root','$record.root=Identity $Root'),('new','$record.staged=Identity $New')]:
 assert source.count(needle)==1
 pause=";if($env:OTTRIN_QA_PAUSE_AT -ceq '"+point+"') { $checkpoint=$env:OTTRIN_QA_CHECKPOINT_FILE+'.pending';[IO.File]::WriteAllText($checkpoint,(@{point='"+point+"';transaction=$record;plugin_script=$env:OTTRIN_INSTALLER_PAYLOAD_SCRIPT}|ConvertTo-Json -Depth 14),[Text.UTF8Encoding]::new($false));[IO.File]::Move($checkpoint,$env:OTTRIN_QA_CHECKPOINT_FILE);while($true){Start-Sleep -Milliseconds 100} }"
 source=source.replace(needle,needle+pause)
helper=ev/'payload-transaction.qa.ps1';helper.write_text(source);out=tmp/'pause.exe';nsi=r/'packaging/windows/installer.nsi'
with (ev/'qa-build.log').open('w') as log:subprocess.run(['makensis','-V2','-DVERSION=0.1.0','-DSTAGE='+str(stage),'-DPAYLOAD_TRANSACTION_SCRIPT='+str(helper),'-DOUTPUT='+str(out),str(nsi)],check=True,stdout=log,stderr=subprocess.STDOUT)
(ev/'QA_BUILD.json').write_text(json.dumps({'scope':'QA checkpoints only in Initialize; actual51-file payload, actual controller and unmodified NSIS authoring','source_revision':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'source_sha256':{str(p.relative_to(r)):hashlib.sha256(p.read_bytes()).hexdigest() for p in [nsi,r/'packaging/windows/payload-transaction.ps1']},'fixture_sha256':{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in [helper,ev/'actor.ps1']},'qa_installer_sha256':hashlib.sha256(out.read_bytes()).hexdigest()},indent=2)+'\n')
