$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$r='C:\OttrinQA-20261011\recovery-current'
$f=Get-ChildItem $r -Directory|Where-Object Name -like 'rollback-candidate-moved Space*'
if(@($f).Count -ne 1){throw 'Expected one timed-out fixture'}
$target=Join-Path $f.FullName 'install'
& "$r\payload-transaction.qa.ps1" -Phase RecoveryPlan -Target $target
$record=Get-Content "$target.ottrin-setup\transaction.json" -Raw|ConvertFrom-Json
if($record.state -cne 'committed'){throw 'Timed-out fixture does not have a committed recovery decision'}
if(Test-Path "$target.ottrin-setup\index-transaction.json"){throw 'Unexpected pending scheduler record'}
& "$r\payload-transaction.qa.ps1" -Phase Commit -Target $target
Copy-Item "$target\Uninstall.exe" "$r\reset-uninstaller.exe"
$p=Start-Process "$r\reset-uninstaller.exe" -ArgumentList "/S _?=$target" -PassThru;$null=$p.Handle
try{if(-not $p.WaitForExit(120000)){Stop-Process -Id $p.Id -Force;throw 'Reset uninstall timeout'};$p.WaitForExit();if($p.ExitCode -ne 0){throw 'Reset uninstall failed'}}finally{$p.Dispose()}
Remove-Item "$r\reset-uninstaller.exe"
Remove-Item $f.FullName -Recurse -Force
if(Test-Path "$r\failure.txt"){Move-Item "$r\failure.txt" "$r\TIMEOUT_FAILURE.txt" -Force}
if(Test-Path "$env:LOCALAPPDATA\ottrin-recovery-qa"){Remove-Item "$env:LOCALAPPDATA\ottrin-recovery-qa" -Recurse -Force}
@{status='pass';disposal_scope='Only timed-out rollback-candidate-moved fixture; persisted commit finished before copied uninstaller ran; six passing fixture cases retained in PROGRESS.json'}|ConvertTo-Json|Set-Content "$r\RESET_REMAINING.json"
