from pathlib import Path
import subprocess,hashlib,json
root=Path.cwd();ev=root/'.hermes/evidence/windows-full-recovery-current';tmp=root/'.hermes/packaging-tmp/full-recovery-current';tmp.mkdir(exist_ok=True)
revision='3ddb1bf65ee514d1ecc69745df66055d2ae515e1';stage=root/'.hermes/packaging-tmp/windows-current/windows-payload/Ottrin-0.1.0-windows-x64'
source=subprocess.check_output(['git','show',revision+':packaging/windows/installer.nsi']).decode();helper=subprocess.check_output(['git','show',revision+':packaging/windows/payload-transaction.ps1']).decode()
assert source==(root/'packaging/windows/installer.nsi').read_text();assert helper==(root/'packaging/windows/payload-transaction.ps1').read_text()
checkpoint=r'''
function Checkpoint($name) {
 if($env:OTTRIN_QA_PAUSE_AT -ceq $name) {
  $data=@{point=$name;pid=$PID;transaction=$record}
  [IO.File]::WriteAllText($env:OTTRIN_QA_CHECKPOINT_FILE,($data|ConvertTo-Json -Depth 14 -Compress),[Text.UTF8Encoding]::new($false))
  while($true){Start-Sleep -Milliseconds 100}
 }
}
'''
helper=helper.replace('$Menu=Join-Path',checkpoint+'\n$Menu=Join-Path',1)
for a,b in [
 ('[IO.Directory]::Move($Target,$Old);Expect $Old $record.previous}',"[IO.Directory]::Move($Target,$Old);Expect $Old $record.previous;Checkpoint 'old-moved'}"),
 ("    Expect $Target $record.candidate\n    $record.state='published';Save $record", "    Expect $Target $record.candidate\n    Checkpoint 'candidate-moved'\n    $record.state='published';Save $record"),
 ("    $record.state='rolled-back';Save $record", "    $record.state='rolled-back';Save $record\n    Checkpoint 'rolled-back'"),
 ("    $record.state='committed';Save $record", "    $record.state='committed';Save $record\n    Checkpoint 'committed'"),
]:
 assert helper.count(a)==1,a;helper=helper.replace(a,b)
(ev/'payload-transaction.qa.ps1').write_text(helper)
(ev/'pause.nsi').write_text(source)
needle='  ; From the seal attempt onward,';assert source.count(needle)==1
(ev/'rollback.nsi').write_text(source.replace(needle,'  Goto index_commit_failed\n'+needle))
receipt={'payload_source_revision':revision,'source_sha256':{},'fixture_sha256':{},'artifacts':{},'scope':'QA-only checkpoints with exact current51-file payload and real embedded index controller; exact unmodified current installer retries recovery'}
for label in ['pause','rollback']:
 out=tmp/(label+'.exe')
 with (ev/(label+'-build.log')).open('w') as log:
  subprocess.run(['makensis','-V2','-DVERSION=0.1.0','-DSTAGE='+str(stage),'-DPAYLOAD_TRANSACTION_SCRIPT='+str(ev/'payload-transaction.qa.ps1'),'-DOUTPUT='+str(out),str(ev/(label+'.nsi'))],check=True,stdout=log,stderr=subprocess.STDOUT)
 receipt['artifacts'][out.name]=hashlib.sha256(out.read_bytes()).hexdigest()
for p in [root/'packaging/windows/installer.nsi',root/'packaging/windows/payload-transaction.ps1']:
 receipt['source_sha256'][str(p.relative_to(root))]=hashlib.sha256(p.read_bytes()).hexdigest()
for p in ev.iterdir():
 if p.suffix in ['.py','.ps1','.nsi']:receipt['fixture_sha256'][p.name]=hashlib.sha256(p.read_bytes()).hexdigest()
(ev/'BUILD.json').write_text(json.dumps(receipt,indent=2)+'\n')
