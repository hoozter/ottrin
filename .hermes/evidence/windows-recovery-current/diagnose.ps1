$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$r='C:\OttrinQA-20261011\recovery-current';$f=Get-ChildItem $r -Directory|Select-Object -First 1
$target=Join-Path $f.FullName 'install';$env:OTTRIN_QA_CONTROL_LOG=Join-Path $f.FullName 'diagnostic-control.log'
$p=Start-Process 'C:\OttrinQA-20261011\recovery-diagnostic.exe' -ArgumentList "/S /D=$target" -PassThru;$null=$p.Handle
try{if(-not $p.WaitForExit(45000)){Stop-Process -Id $p.Id -Force;throw 'Diagnostic timeout'};$p.WaitForExit();[IO.File]::WriteAllText("$r\DIAGNOSTIC_EXIT.txt",[string]$p.ExitCode)}finally{$p.Dispose()}
