from pathlib import Path
import subprocess,shutil,hashlib,json
root=Path.cwd();ev=root/'.hermes/evidence/windows-recovery-current';tmp=root/'.hermes/packaging-tmp/recovery-current';tmp.mkdir(parents=True,exist_ok=True)
def alias(s):
 return s.replace('Software\\Ottrin"','Software\\OttrinRecoveryQA"').replace('Uninstall\\Ottrin"','Uninstall\\OttrinRecoveryQA"').replace('$SMPROGRAMS\\Ottrin','$SMPROGRAMS\\OttrinRecoveryQA').replace('$LOCALAPPDATA\\ottrin\\','$LOCALAPPDATA\\ottrin-recovery-qa\\').replace('Global\\OttrinInstaller','Global\\OttrinRecoveryQAInstaller')
baseline=(root/'.hermes/evidence/windows-payload-current/baseline.nsi').read_text().replace('OttrinPayloadQA','OttrinRecoveryQA').replace('ottrin-payload-qa','ottrin-recovery-qa').replace('Global\\OttrinInstaller','Global\\OttrinRecoveryQAInstaller')
(ev/'baseline.nsi').write_text(baseline)
fixed=alias((root/'packaging/windows/installer.nsi').read_text()).replace('  Pop $PayloadTransactionResult\n', '  Pop $PayloadTransactionResult\n  FileOpen $2 "$EXEDIR\\phase-$PayloadTransactionPhase.txt" w\n  FileWriteUTF16LE $2 "$PayloadTransactionPhase|$0|$PayloadTransactionResult$\\r$\\n"\n  FileClose $2\n')
(ev/'fixed.nsi').write_text(fixed)
s=(root/'packaging/windows/payload-transaction.ps1').read_text().replace("path='Software\\Ottrin'","path='Software\\OttrinRecoveryQA'").replace("Uninstall\\Ottrin'","Uninstall\\OttrinRecoveryQA'").replace("'Programs')) 'Ottrin'","'Programs')) 'OttrinRecoveryQA'")
qa=r'''
function Checkpoint($name) {
 if($env:OTTRIN_QA_PAUSE_AT -ceq $name) {
  [IO.File]::WriteAllText($env:OTTRIN_QA_CHECKPOINT_FILE,(@{point=$name;pid=$PID;root=$Root;state=$record.state;transaction=$record;live=$(if(Test-Path -LiteralPath $Target){Identity $Target}else{$null})}|ConvertTo-Json -Depth 12 -Compress))
  while($true){Start-Sleep -Milliseconds 100}
 }
}
'''
s=s.replace('$Menu=Join-Path',qa+'\n$Menu=Join-Path',1)
s=s.replace("if ($Phase -eq 'Initialize') {",r'''if ($Phase -eq 'Initialize') {
    if($env:OTTRIN_QA_OBSERVATION) {
        $observation=@{live=$(if(Test-Path -LiteralPath $Target){Identity $Target}else{$null});version=$(if(Test-Path "$Target\version.txt"){[IO.File]::ReadAllText("$Target\version.txt")}else{$null});registry=@(foreach($spec in $RegistrySpec){RegistrySnapshot $spec});links=@(foreach($name in $Links){$path=Join-Path $Menu $name;@{name=$name;bytes=$(if(Test-Path $path){[Convert]::ToBase64String([IO.File]::ReadAllBytes($path))}else{$null})}})}
        [IO.File]::WriteAllText($env:OTTRIN_QA_OBSERVATION,($observation|ConvertTo-Json -Depth 10 -Compress))
    }''',1)
changes=[
("        Expect $Completed $record.root\n        $completedJournal=", "        Expect $Completed $record.root\n        Checkpoint 'final-receipt'\n        $completedJournal="),
("        [IO.Directory]::Delete($Completed)\n", "        Checkpoint 'final-empty'\n        [IO.Directory]::Delete($Completed)\n"),
("        Save $record\n    } catch {","        Save $record\n        Checkpoint 'staging'\n    } catch {"),
("    $record.state='publishing';Save $record","    $record.state='publishing';Save $record\n    Checkpoint 'publishing'"),
("[IO.Directory]::Move($Target,$Old);Expect $Old $record.previous}", "[IO.Directory]::Move($Target,$Old);Expect $Old $record.previous;Checkpoint 'old-moved'}"),
("    Expect $Target $record.candidate\n    $record.state='published';Save $record", "    Expect $Target $record.candidate\n    Checkpoint 'candidate-moved'\n    $record.state='published';Save $record\n    Checkpoint 'published'"),
("    $record.state='integrating';Save $record", "    $record.state='integrating';Save $record\n    Checkpoint 'integrating'"),
("        [IO.Directory]::Move($Target,$New)\n", "        [IO.Directory]::Move($Target,$New)\n        Checkpoint 'rollback-candidate-moved'\n"),
("        [IO.Directory]::Move($Old,$Target)\n", "        [IO.Directory]::Move($Old,$Target)\n        Checkpoint 'rollback-old-restored'\n"),
("    $record.state='rolled-back';Save $record", "    $record.state='rolled-back';Save $record\n    Checkpoint 'rolled-back'"),
("    $record.state='committed';Save $record", "    $record.state='committed';Save $record\n    Checkpoint 'committed'"),
("                if ((Test-Path -LiteralPath $path) -and -not (Retire $path $entry.previous)) {throw \"Previous payload changed and is retained: $path\"}", "                if ((Test-Path -LiteralPath $path) -and -not (Retire $path $entry.previous)) {throw \"Previous payload changed and is retained: $path\"}\n                Checkpoint 'cleanup-leaf'"),
("        EmptyDirectories $Old\n", "        EmptyDirectories $Old\n        Checkpoint 'cleanup-tree'\n"),
]
for a,b in changes:
 assert s.count(a)==1,a;s=s.replace(a,b)
(ev/'payload-transaction.qa.ps1').write_text(s)
mock=(root/'.hermes/evidence/windows-commit-current/mock.c').read_text().replace('if(!DeleteFileW(argv[3]))return 14;', 'if(!DeleteFileW(argv[3])&&GetLastError()!=ERROR_FILE_NOT_FOUND)return 14;')
(ev/'mock.c').write_text(mock)
build={'production_sha256':{},'fixture_sha256':{},'artifacts':{}}
for name in ['packaging/windows/installer.nsi','packaging/windows/payload-transaction.ps1']:
 build['production_sha256'][name]=hashlib.sha256((root/name).read_bytes()).hexdigest()
for label,version in [('baseline','old'),('fixed','new')]:
 stage=tmp/label/'stage';(stage/'bin').mkdir(parents=True,exist_ok=True)
 subprocess.run(['x86_64-w64-mingw32-gcc','-municode','-DPAYLOAD_VERSION=L"'+version+'"',str(ev/'mock.c'),'-o',str(stage/'bin/ottrin.exe')],check=True)
 shutil.copyfile(stage/'bin/ottrin.exe',stage/'bin/ottrin-indexd.exe');(stage/'version.txt').write_text(version)
 exe=tmp/(label+'.exe')
 subprocess.run(['makensis','-V2','-DSTAGE='+str(stage),'-DVERSION='+('0.1.0' if label=='baseline' else '0.2.0'),'-DPAYLOAD_TRANSACTION_SCRIPT='+str(ev/'payload-transaction.qa.ps1'),'-DOUTPUT='+str(exe),str(ev/(label+'.nsi'))],check=True)
 build['artifacts'][exe.name]=hashlib.sha256(exe.read_bytes()).hexdigest()
for p in ev.iterdir():
 if p.suffix in ['.py','.ps1','.nsi','.c']:build['fixture_sha256'][p.name]=hashlib.sha256(p.read_bytes()).hexdigest()
(ev/'BUILD.json').write_text(json.dumps(build,indent=2)+'\n')
