$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$r='C:\OttrinQA-20261011\recovery-current'
foreach($f in (Get-ChildItem $r -Directory)){
 $target=Join-Path $f.FullName 'install'
 if(Test-Path "$target.ottrin-setup"){throw "Unexpected active record: $target"}
 if(Test-Path "$target.ottrin-completed"){throw "Unexpected completed record: $target"}
 if(Test-Path "$target\Uninstall.exe"){
  Copy-Item "$target\Uninstall.exe" "$r\reset-uninstaller.exe" -Force
  $p=Start-Process "$r\reset-uninstaller.exe" -ArgumentList "/S _?=$target" -PassThru;$null=$p.Handle
  try{if(-not $p.WaitForExit(120000)){Stop-Process -Id $p.Id -Force;throw 'Reset uninstall timeout'};$p.WaitForExit();if($p.ExitCode -ne 0){throw 'Reset uninstall failed'}}finally{$p.Dispose()}
  Remove-Item "$r\reset-uninstaller.exe"
 }
 Remove-Item $f.FullName -Recurse -Force
}
foreach($name in @('PROGRESS.json','failure.txt')){if(Test-Path "$r\$name"){Remove-Item "$r\$name"}}
if(Test-Path "$env:LOCALAPPDATA\ottrin-recovery-qa"){Remove-Item "$env:LOCALAPPDATA\ottrin-recovery-qa" -Recurse -Force}
if(Test-Path 'HKCU:\Software\OttrinRecoveryQA'){throw 'Fixture registration remains'}
@{status='pass';scope='Only owned recovery-current case directories and isolated integrations reset; prior evidence collected on host'}|ConvertTo-Json|Set-Content "$r\FULL_RESET.json"
