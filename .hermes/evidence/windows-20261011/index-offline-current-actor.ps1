param([Parameter(Mandatory=$true)][ValidatePattern('^[a-f0-9]{64}$')][string]$ExpectedPackageSha256)
$ProgressPreference='SilentlyContinue';$ErrorActionPreference='Stop'
$b='C:\OttrinQA-20261011';$r="$b\index-offline-current";$stage="$b\index-offline-payload\Ottrin-0.1.0-windows-x64"
$app="$stage\bin\ottrin.exe";$daemon="$stage\bin\ottrin-indexd.exe";$result=@{status='running'}
$env:APPDATA="$r\roaming";$env:LOCALAPPDATA="$r\local";$env:OTTRIN_DEV_HELPER_AUTOBUILD='0'
function Assert($ok,$message){if(-not $ok){throw $message}}
$script:cliSequence=0
function Invoke-App($arguments){
 $script:cliSequence++
 $arguments|Set-Content "$script:r\phase.txt"
 $stdout="$script:r\cli-$script:cliSequence.stdout";$stderr="$script:r\cli-$script:cliSequence.stderr"
 $p=Start-Process $script:app -ArgumentList $arguments -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
 $null=$p.Handle
 try{
  "wait $arguments"|Set-Content "$script:r\phase.txt"
  Assert ($p.WaitForExit(30000)) "CLI timeout $arguments"
  "read $arguments"|Set-Content "$script:r\phase.txt"
  $err=[IO.File]::ReadAllText($stderr)
  Assert ($p.ExitCode -eq 0) "CLI failure $arguments : $err"
  return [IO.File]::ReadAllText($stdout)
 }finally{"dispose $arguments"|Set-Content "$script:r\phase.txt";if(-not $p.HasExited){$p.Kill();$null=$p.WaitForExit(5000)};$p.Dispose();"returned $arguments"|Set-Content "$script:r\phase.txt"}
}
function Checkpoint($phase){
 $result.phase=$phase
 [IO.File]::WriteAllText("$r\checkpoint.json",($result|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false))
}
function Query($term){return ((Invoke-App "search $term --json")|ConvertFrom-Json)}
function Quiet($p){
 "quiet CPU before"|Set-Content "$script:r\phase.txt"
 $before=$p.TotalProcessorTime.TotalSeconds;"quiet sleeping"|Set-Content "$script:r\phase.txt";$clock=[Diagnostics.Stopwatch]::StartNew()
 Start-Sleep -Seconds 15;"quiet check exit"|Set-Content "$script:r\phase.txt";Assert (-not $p.HasExited) 'Daemon exited during quiet sample'
 "quiet CPU after"|Set-Content "$script:r\phase.txt"
 return @{seconds=$clock.Elapsed.TotalSeconds;cpu_percent_one_core=100*($p.TotalProcessorTime.TotalSeconds-$before)/$clock.Elapsed.TotalSeconds}
}
try {
 Assert ($env:USERNAME -eq 'OttrinIndexQA') 'Wrong account'
 $id=[Security.Principal.WindowsIdentity]::GetCurrent();Assert (-not ([Security.Principal.WindowsPrincipal]::new($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Elevated actor'
 $scheduler=New-Object -ComObject Schedule.Service;$scheduler.Connect();$folder=$scheduler.GetFolder('\')
 $absent=$false;try{$null=$folder.GetTask('Ottrin Index '+$id.User.Value)}catch{if($_.Exception.GetBaseException().HResult -eq -2147024894){$absent=$true}else{throw}}
 Assert $absent 'Unexpected installed index task'
 Assert ((Get-FileHash "$b\index-offline-portable.zip").Hash.ToLowerInvariant() -eq $ExpectedPackageSha256) 'Wrong package'
 $result.package_sha256=$ExpectedPackageSha256;$result.source_record=[IO.File]::ReadAllText("$stage\SOURCE_REVISION.txt")
 New-Item "$r\roaming\ottrin","$r\local","$r\corpus" -ItemType Directory -Force|Out-Null
 $config=Get-Content 'C:\Users\OttrinIndexQA\AppData\Roaming\ottrin\config.json' -Raw|ConvertFrom-Json
 $config.search.include_roots=@("$r\corpus");$config.search.refresh_interval_hours=0;$config.search.content_indexing_enabled=$false;$config.search.use_system_locate=$false;$config.search.manage_locate_db=$false
 [IO.File]::WriteAllText("$r\roaming\ottrin\config.json",($config|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false))
 for($bucket=0;$bucket -lt 100;$bucket++){
  $dir=Join-Path "$r\corpus" ('bucket-{0:d3}' -f $bucket);New-Item $dir -ItemType Directory|Out-Null
  for($item=0;$item -lt 100;$item++){[IO.File]::WriteAllText((Join-Path $dir ('seed-{0:d5}.txt' -f ($bucket*100+$item))),'original fixture')}
 }
 $p=Start-Process $daemon -PassThru;$null=$p.Handle
 $null=Invoke-App 'index update --json';$seed=Query 'seed-';Assert ($seed.total -eq 10000) 'Initial fixture count differs';$result.initial_query_total=$seed.total
 "initial check exit"|Set-Content "$r\phase.txt";Assert (-not $p.HasExited) 'Direct initial daemon was replaced';$result.initial_pid=$p.Id;Start-Sleep -Seconds 2
 $result.before_idle=Quiet $p;Checkpoint 'initial idle measured';Assert ($result.before_idle.cpu_percent_one_core -lt 0.5) 'Populated idle CPU exceeds budget'
 $result.stop_output=Invoke-App 'index service-remove';Assert ($p.WaitForExit(10000)) 'Daemon did not stop';$result.stopped_exit=$p.ExitCode;Assert ($p.ExitCode -eq 0) 'Daemon stop nonzero'
 $database="$r\local\ottrin\search.db";Assert (Test-Path $database) 'Stopping deleted persisted index'
 $modified="$r\corpus\bucket-020\seed-02000.txt";$newText='changed while daemon stopped with different metadata size'
 [IO.File]::WriteAllText($modified,$newText)
 Rename-Item "$r\corpus\bucket-030\seed-03000.txt" 'renamed-offline.txt'
 Remove-Item "$r\corpus\bucket-040\seed-04000.txt"
 [IO.File]::WriteAllText("$r\corpus\bucket-099\born-offline.txt",'created offline')
 $clock=[Diagnostics.Stopwatch]::StartNew();$p=Start-Process $daemon -PassThru;$null=$p.Handle
 $deadline=[DateTime]::UtcNow.AddSeconds(30)
 do {
  $renamed=Query 'renamed-offline';$born=Query 'born-offline';$old=Query 'seed-03000';$removed=Query 'seed-04000';$edit=Query 'seed-02000'
  $fresh=$renamed.total -eq 1 -and $born.total -eq 1 -and $old.total -eq 0 -and $removed.total -eq 0 -and $edit.total -eq 1 -and $edit.items[0].size_bytes -eq [Text.Encoding]::UTF8.GetByteCount($newText)
  if($fresh){break};Start-Sleep -Milliseconds 100
 }while([DateTime]::UtcNow -lt $deadline)
 Assert $fresh 'Offline mutations not reconciled'
 $result.offline_freshness_ms=$clock.Elapsed.TotalMilliseconds;$result.offline_queries=@{renamed=$renamed;created=$born;old_name=$old;deleted=$removed;modified=$edit}
 Assert (-not $p.HasExited) 'CLI replaced directly started daemon'
 $result.restart_pid=$p.Id;$result.index_status=(Invoke-App 'index status --json')|ConvertFrom-Json;Assert $result.index_status.watcher_ready 'Restart watch not ready'
 Start-Sleep -Seconds 2;$result.after_idle=Quiet $p;Checkpoint 'offline freshness and restart idle measured';Assert ($result.after_idle.cpu_percent_one_core -lt 0.5) 'Post-restart idle CPU exceeds budget'
 $result.cleanup_output=Invoke-App '--uninstall';Checkpoint 'cleanup CLI returned';Assert ($p.WaitForExit(10000)) 'Cleanup did not stop daemon';Assert ($p.ExitCode -eq 0) 'Cleanup stop nonzero';Assert (-not(Test-Path $database)) 'Index remains after cleanup'
 $result.status='pass';Checkpoint 'all assertions passed'
}catch{$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally {
 if($p -and -not $p.HasExited){$p.Kill();$null=$p.WaitForExit(5000)}
}
$result|ConvertTo-Json -Depth 30|Set-Content "$r\result.json" -Encoding UTF8
if($result.status -ne 'pass'){exit 1};exit 0
