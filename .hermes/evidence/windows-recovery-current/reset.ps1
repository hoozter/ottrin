$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$r='C:\OttrinQA-20261011\recovery-current'
$folder=Join-Path $r "staging Space Ω 'copy'";$target=Join-Path $folder 'install'
if(Test-Path "$target.ottrin-setup"){throw 'Unexpected pending transaction in rejected initial run'}
if(Test-Path "$target\Uninstall.exe"){
 Copy-Item "$target\Uninstall.exe" "$r\reset-uninstaller.exe"
 $p=Start-Process "$r\reset-uninstaller.exe" -ArgumentList "/S _?=$target" -PassThru;$null=$p.Handle
 try{if(-not $p.WaitForExit(30000)){Stop-Process -Id $p.Id -Force;throw 'Reset uninstall timeout'};$p.WaitForExit();if($p.ExitCode -ne 0){throw 'Reset uninstall failed'}}finally{$p.Dispose()}
 Remove-Item "$r\reset-uninstaller.exe"
}
if(Test-Path $folder){Remove-Item $folder -Recurse -Force}
if(Test-Path "$r\failure.txt"){Move-Item "$r\failure.txt" "$r\LATEST_REJECTED_FAILURE.txt" -Force}
if(Test-Path "$env:LOCALAPPDATA\ottrin-recovery-qa"){Remove-Item "$env:LOCALAPPDATA\ottrin-recovery-qa" -Recurse -Force}
if(Test-Path 'HKCU:\Software\OttrinRecoveryQA'){throw 'Fixture registry remains'}
[IO.File]::WriteAllText("$r\RESET.txt",'done')
