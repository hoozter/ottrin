param([switch]$Fixed)
$ProgressPreference='SilentlyContinue';$ErrorActionPreference='Stop'
$b='C:\OttrinQA-20261011';$r="$b\move-gui-current";$result=@{status='running';fixed=[bool]$Fixed}
$env:APPDATA="$r\roaming";$env:LOCALAPPDATA="$r\local"
$exe=if($Fixed){"$b\session-cleanup-fixed.exe"}else{"$b\move-gui-payload\Ottrin-0.1.0-windows-x64\bin\ottrin.exe"}
try {
 $start=[Diagnostics.ProcessStartInfo]::new($exe,'--uninstall');$start.UseShellExecute=$false;$start.RedirectStandardOutput=$true;$start.RedirectStandardError=$true
 $p=[Diagnostics.Process]::Start($start);$null=$p.Handle;$out=$p.StandardOutput.ReadToEndAsync();$err=$p.StandardError.ReadToEndAsync()
 if(-not $p.WaitForExit(30000)){throw 'Cleanup CLI timeout'}
 $result.exit=$p.ExitCode;$result.stdout=$out.Result;$result.stderr=$err.Result
 $result.database_present=Test-Path "$r\local\ottrin\search.db"
 $result.daemon_pids=@(Get-CimInstance Win32_Process -Filter "Name='ottrin-indexd.exe'"|Where-Object {$_.ExecutablePath -like "$b\move-gui-payload\*"}|ForEach-Object ProcessId)
 $result.executable_sha256=(Get-FileHash $exe).Hash.ToLower()
 if($Fixed){if($p.ExitCode -ne 0 -or $result.database_present -or $result.daemon_pids.Count){throw 'Fixed cleanup incomplete'}}
 else{if($p.ExitCode -ne 1 -or -not $result.database_present -or $result.daemon_pids.Count -ne 1){throw 'Expected unregistered-session failure not reproduced'}}
 $result.status='pass'
}catch{$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
$name=if($Fixed){'cleanup-fixed.json'}else{'cleanup-baseline.json'}
$result|ConvertTo-Json -Depth 8|Set-Content "$r\$name" -Encoding UTF8
if($result.status -ne 'pass'){exit 1};exit 0
