$ErrorActionPreference='Stop';$result=@{status='running';suites=@{}};$owned=@();$temp=$null
try {
 if($env:USERNAME-ne'OttrinContentQA'){throw 'Wrong account'}
 $id=[Security.Principal.WindowsIdentity]::GetCurrent();if(([Security.Principal.WindowsPrincipal]::new($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Elevated actor'}
 $result.ordinary_user=$true;$env:OTTRIN_REQUIRE_EPS_RUNTIME='1';$temp=Join-Path $env:TEMP 'op-Eps% Rendering';if(Test-Path $temp){throw 'Owned namespace exists'};New-Item -ItemType Directory $temp|Out-Null
 foreach($suite in @('preview','eps','job','ui')){
  $dir=Join-Path $temp $suite;New-Item -ItemType Directory $dir|Out-Null;$env:TEMP=$dir;$env:TMP=$dir
  $p=Start-Process "C:\OttrinContentQA\$suite.exe" -ArgumentList '--nocapture --test-threads=1' -WorkingDirectory C:\Windows\System32 -PassThru -RedirectStandardOutput "C:\OttrinContentQA\results\$suite.stdout" -RedirectStandardError "C:\OttrinContentQA\results\$suite.stderr"
  $null=$p.Handle;$owned+=,$p;if(-not$p.WaitForExit(240000)){throw "Owned suite live: $($p.Id)"}
  $result.suites[$suite]=@{exit=$p.ExitCode;temporary_entries=@(Get-ChildItem $dir -Force).Count}
  Remove-Item $dir -Recurse -Force
 }
 $result.status='collected'
} catch {$result.status='fail';$result.error=$_.Exception.Message}
finally {
 foreach($p in $owned){if(-not$p.HasExited){& C:\Windows\System32\taskkill.exe /PID $p.Id /T /F|Out-Null;$null=$p.WaitForExit(10000)}}
 if($temp-and(Test-Path $temp)){Remove-Item $temp -Recurse -Force}
 $result|ConvertTo-Json -Depth 6|Set-Content C:\OttrinContentQA\results\RESULTS.json
}
if($result.status-ne'collected'){exit 1}
