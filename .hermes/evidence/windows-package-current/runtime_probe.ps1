$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$root='C:\OttrinPackageQA';$profile="$root\runtime-profile"
$payload="$root\payload\Ottrin-0.1.0-windows-x64"
$app="$payload\bin\ottrin.exe";$daemon="$payload\bin\ottrin-indexd.exe"
$result=@{status='running';scope='Exact portable payload native SYSTEM/private-profile CLI, watcher, persistence and bounded quiet-idle acceptance; not interactive-user service or UI acceptance'}
function Assert($c,$m){if(-not $c){throw $m}}
function Daemons { @(Get-CimInstance Win32_Process -Filter "Name='ottrin-indexd.exe'"|Where-Object {$_.ExecutablePath -eq $daemon}) }
function Invoke-App([string]$arguments){
 $info=New-Object Diagnostics.ProcessStartInfo($app,$arguments);$info.UseShellExecute=$false;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true;$info.CreateNoWindow=$true
 $p=[Diagnostics.Process]::Start($info)
 try {$out=$p.StandardOutput.ReadToEndAsync();$err=$p.StandardError.ReadToEndAsync();Assert ($p.WaitForExit(45000)) "CLI timeout PID $($p.Id)";Assert ($out.Wait(5000) -and $err.Wait(5000)) "CLI output remained open after exit: $arguments";Assert ($p.ExitCode -eq 0) "CLI error $($p.ExitCode): $arguments : $($err.Result)";return $out.Result}
 finally {if(-not $p.HasExited){$p.Kill();$p.WaitForExit()};$p.Dispose()}
}
try {
 Assert (@(Daemons).Count -eq 0) 'Existing consumer of owned portable stage'
 Assert (-not (Test-Path $profile)) 'Runtime profile already exists'
 foreach($path in @("$profile\roaming\ottrin","$profile\local","$profile\fixtures")){New-Item -ItemType Directory $path -Force|Out-Null}
 $env:APPDATA="$profile\roaming";$env:LOCALAPPDATA="$profile\local";$env:USERPROFILE=$profile;$env:HOME=$profile
 $config=Get-Content "$root\runtime-config.json" -Raw | ConvertFrom-Json
 $config.search.include_roots=@("$profile\fixtures");$config.search.exclude_roots=@();$config.search.content_indexing_enabled=$false
 $config.bookmarks=@();$config.last_session_dirs=@();$config|ConvertTo-Json -Depth 30|Set-Content "$profile\roaming\ottrin\config.json" -Encoding UTF8
 # UTF-8 without BOM is the production JSON format.
 $json=Get-Content "$profile\roaming\ottrin\config.json" -Raw;[IO.File]::WriteAllText("$profile\roaming\ottrin\config.json",$json,(New-Object Text.UTF8Encoding($false)))
 [IO.File]::WriteAllText("$profile\fixtures\ottrin-package-fixture.txt",'native current package fixture')
 $watch=[Diagnostics.Stopwatch]::StartNew();$result.cold_update=Invoke-App 'index update --json';$result.cold_update_ms=$watch.ElapsedMilliseconds
 $workers=@(Daemons);Assert ($workers.Count -eq 1) 'Expected one daemon after cold update';$daemonId=$workers[0].ProcessId
 $query=(Invoke-App 'search ottrin-package-fixture --json')|ConvertFrom-Json;Assert ($query.total -eq 1) 'Initial file absent from index'
 $result.repeated_update=Invoke-App 'index update --json';$workers=@(Daemons);Assert ($workers.Count -eq 1 -and $workers[0].ProcessId -eq $daemonId) 'Repeated CLI replaced or duplicated daemon'
 Start-Sleep -Seconds 2
 $process=Get-Process -Id $daemonId;$before=$process.TotalProcessorTime.TotalMilliseconds;$wall=[Diagnostics.Stopwatch]::StartNew();Start-Sleep -Seconds 8;$process.Refresh();$elapsed=$wall.Elapsed.TotalMilliseconds;$cpu=$process.TotalProcessorTime.TotalMilliseconds-$before
 $result.quiet_idle=@{wall_ms=$elapsed;cpu_ms=$cpu;one_core_percent=100*$cpu/$elapsed;working_set_bytes=$process.WorkingSet64};Assert ($cpu/$elapsed -lt .02) 'Quiet daemon exceeds 2% of one CPU core'
 [IO.File]::WriteAllText("$profile\fixtures\ottrin-package-watched.txt",'watcher fixture')
 $limit=[DateTime]::UtcNow.AddSeconds(20);$found=$false
 do {$query=(Invoke-App 'search ottrin-package-watched --json')|ConvertFrom-Json;if($query.total -eq 1){$found=$true;break};Start-Sleep -Milliseconds 200}while([DateTime]::UtcNow -lt $limit)
 Assert $found 'Watcher did not discover created file';$result.watcher_added_file=$true
 $p=Get-Process -Id $daemonId;$p.Kill();$p.WaitForExit();$p.Dispose()
 $result.restart_update=Invoke-App 'index update --json';$query=(Invoke-App 'search ottrin-package --json')|ConvertFrom-Json;Assert ($query.total -eq 2) 'Persisted restart missing expected files'
 $workers=@(Daemons);Assert ($workers.Count -eq 1 -and $workers[0].ProcessId -ne $daemonId) 'Expected fresh singleton daemon after restart'
 $result.daemon_reused_pid=$daemonId;$result.restarted_pid=$workers[0].ProcessId;$result.persisted_restart_files=$query.total
 $result.status='pass'
} catch {$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally {
 foreach($worker in @(Daemons)){$p=Get-Process -Id $worker.ProcessId -ErrorAction SilentlyContinue;if($p){$p.Kill();$p.WaitForExit();$p.Dispose()}}
 $result.cleanup_processes_absent=(@(Daemons).Count -eq 0)
 if(Test-Path $profile){Remove-Item $profile -Recurse -Force};$result.cleanup_profile_absent=(-not(Test-Path $profile))
 $result|ConvertTo-Json -Depth 12|Set-Content "$root\runtime-result.json"
}
if($result.status -ne 'pass'){exit 1}
