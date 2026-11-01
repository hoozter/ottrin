 # This block runs only inside the owned ordinary-token fixture profile.
 $prior=$null;$ensure=$null;$newPeer=$null
 function IndexPing([int]$version) {
  $pipe=[IO.Pipes.NamedPipeClientStream]::new('.','Ottrin.Index.'+$identity.User.Value,[IO.Pipes.PipeDirection]::InOut,[IO.Pipes.PipeOptions]::Asynchronous)
  try {
   $pipe.Connect(1500)
   $bytes=[Text.Encoding]::UTF8.GetBytes('{"version":'+$version+',"request":{"type":"ping"}}')
   $header=[BitConverter]::GetBytes([uint32]$bytes.Length);[Array]::Reverse($header);$pipe.Write($header,0,4);$pipe.Write($bytes,0,$bytes.Length);$pipe.Flush()
   $reader=[IO.BinaryReader]::new($pipe,[Text.Encoding]::UTF8,$true)
   try {$header=$reader.ReadBytes(4);if($header.Length-ne4){throw 'Missing frame header'};[Array]::Reverse($header);$length=[BitConverter]::ToUInt32($header,0);if($length-gt1048576){throw 'Oversized frame'};$body=$reader.ReadBytes($length);if($body.Length-ne$length){throw 'Truncated response'};return ([Text.Encoding]::UTF8.GetString($body)|ConvertFrom-Json)} finally {$reader.Dispose()}
  } finally {$pipe.Dispose()}
 }
 try {
  if(@(Get-CimInstance Win32_Process|Where-Object {$_.Name-ieq'ottrin-indexd.exe'}).Count){throw 'Existing index process preserved; fixture requires a free endpoint'}
  $corpus=Join-Path $homeRoot 'corpus';New-Item -ItemType Directory $corpus|Out-Null;[IO.File]::WriteAllText((Join-Path $corpus 'note.txt'),'preserve source')
  $configDir=Join-Path $env:APPDATA 'ottrin';New-Item -ItemType Directory $configDir|Out-Null
  # This entire file is private fixture state; AppConfig fills omitted defaults.
  @{search=@{include_roots=@($corpus);exclude_roots=@();content_indexing_enabled=$false;refresh_interval_hours=0;use_system_locate=$false;manage_locate_db=$false}}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $configDir 'config.json')
  $configHash=(Get-FileHash (Join-Path $configDir 'config.json')).Hash
  $previousDir=Join-Path $homeRoot 'previous';New-Item -ItemType Directory $previousDir|Out-Null;Copy-Item "$root\previous-indexd.exe" "$previousDir\ottrin-indexd.exe"
  $prior=Start-Process "$previousDir\ottrin-indexd.exe" -ArgumentList '--index-daemon' -WorkingDirectory $homeRoot -PassThru;$null=$prior.Handle
  $before=$null;$deadline=[DateTime]::UtcNow.AddSeconds(30)
  do {try {$before=IndexPing 1}catch {if($prior.HasExited){throw 'Previous daemon exited before readiness'};Start-Sleep -Milliseconds 50}} while(-not$before-and[DateTime]::UtcNow-lt$deadline)
  if(-not$before-or$before.version-ne1-or$before.response.type-ne'pong'){throw 'Previous protocol1 daemon never ready'}
  $ensure=Start-Process "$root\fixture-launch.exe" -ArgumentList @('index','ensure') -WorkingDirectory $homeRoot -PassThru -RedirectStandardOutput "$root\results\$phase-upgrade.stdout" -RedirectStandardError "$root\results\$phase-upgrade.stderr";$null=$ensure.Handle
  if(-not$ensure.WaitForExit(45000)){throw 'Current ensure timeout'};if($ensure.ExitCode-ne0){throw ('Current ensure failed: '+(Get-Content "$root\results\$phase-upgrade.stderr" -Raw))}
  if(-not$prior.WaitForExit(5000)){throw 'Previous daemon remains alive'}
  $after=IndexPing 2;if($after.version-ne2-or$after.response.type-ne'pong'){throw 'Current protocol2 daemon not ready'}
  $peers=@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath-ieq"$root\ottrin-indexd.exe"});if($peers.Count-ne1){throw 'Current owned daemon identity missing'};$newPeer=[int]$peers[0].ProcessId
  if((Get-FileHash (Join-Path $configDir 'config.json')).Hash-ne$configHash){throw 'Preferences changed'};if([IO.File]::ReadAllText((Join-Path $corpus 'note.txt'))-ne'preserve source'){throw 'Source changed'}
  @{status='pass';before=$before;after=$after;oldExit=$prior.ExitCode;ensureExit=$ensure.ExitCode;preferencesPreserved=$true;sourcePreserved=$true;newExecutable="$root\ottrin-indexd.exe";newPid=$newPeer;scope='Ordinary token, accepted candidate daemon -> current source daemon; private profile'}|ConvertTo-Json -Depth 8|Set-Content "$root\results\$phase-upgrade.json"
 } finally {
  foreach($owned in @($ensure,$prior)){if($owned-and-not$owned.HasExited){& C:\Windows\System32\taskkill.exe /PID $owned.Id /T /F|Out-Null;$null=$owned.WaitForExit(10000)}}
  foreach($peer in @(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath-ieq"$root\ottrin-indexd.exe"})){& C:\Windows\System32\taskkill.exe /PID $peer.ProcessId /T /F|Out-Null}
 }
