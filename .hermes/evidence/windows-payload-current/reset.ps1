$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$r='C:\OttrinQA-20261011\payload-current'
$cases=@(Get-ChildItem $r -Directory | Where-Object Name -like "* Space * 'copy'")
foreach($case in $cases){
 $target=Join-Path $case.FullName 'install'
 if(Test-Path "$target\Uninstall.exe"){
  if((Get-Content "$target\version.txt" -Raw) -notin @('old','new')){throw 'Unexpected existing fixture payload'}
  $runner=Join-Path $case.FullName 'reset-uninstall.exe';Copy-Item "$target\Uninstall.exe" $runner
  $p=Start-Process $runner -ArgumentList "/S _?=$target" -PassThru;$null=$p.Handle
  if(-not $p.WaitForExit(15000)){throw 'Reset uninstall timeout'};$p.WaitForExit();if($p.ExitCode -ne 0){throw 'Reset uninstall failure'}
 }
 Remove-Item $case.FullName -Recurse -Force
}
$foreign="$r\foreign-directory"
if(Test-Path $foreign){if((Get-Content "$foreign\user-data.txt" -Raw) -cne 'unrelated data'){throw 'Foreign fixture changed'};Remove-Item "$foreign\user-data.txt";Remove-Item $foreign}
$probe="$r\wrapper-probe.ottrin-setup"
if(Test-Path $probe){
 if(@(Get-ChildItem "$probe\new" -Force).Count -ne 0){throw 'Unexpected diagnostic payload'}
 Remove-Item "$probe\new";Remove-Item "$probe\transaction.json";Remove-Item $probe
}
if(Test-Path "$r\debug-install"){throw 'Unexpected debug installation'}
if(Test-Path "$env:LOCALAPPDATA\ottrin-payload-qa"){Remove-Item "$env:LOCALAPPDATA\ottrin-payload-qa" -Recurse -Force}
if(Test-Path "$r\failure.txt"){Move-Item "$r\failure.txt" ("$r\failed-run-"+[Guid]::NewGuid().ToString('N')+'.txt')}
foreach($name in @('RESULTS.json','PROGRESS.json','USER_CLEANUP.json','LEAF.json')){if(Test-Path "$r\$name"){Remove-Item "$r\$name"}}
'Owned initial fixtures reset; diagnostic record removed'|Set-Content "$r\reset.log"
