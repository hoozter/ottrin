param([switch]$ReuseOwnedProfile, [Parameter(Mandatory=$true)][ValidatePattern('^[a-f0-9]{64}$')][string]$ExpectedInstallerSha256)
$ProgressPreference='SilentlyContinue';$ErrorActionPreference='Stop'
$b='C:\OttrinQA-20261011'
$r="$b\nsis-current-lifecycle"
$result=@{status='running';phase='seed'}
function Assert($condition,$message){if(-not $condition){throw $message}}
function Invoke-App([string]$Arguments){
 $start=New-Object Diagnostics.ProcessStartInfo($script:app,$Arguments)
 $start.UseShellExecute=$false;$start.RedirectStandardOutput=$true;$start.RedirectStandardError=$true
 $p=[Diagnostics.Process]::Start($start)
 try {
  $stdout=$p.StandardOutput.ReadToEndAsync();$stderr=$p.StandardError.ReadToEndAsync()
  Assert ($p.WaitForExit(30000)) "CLI still running: $($p.Id)"
  $out=$stdout.Result;$err=$stderr.Result
  Assert ($p.ExitCode -eq 0) "CLI exit $($p.ExitCode): $Arguments : $err"
  return $out
 }finally{if(-not $p.HasExited){$p.Kill();$p.WaitForExit()};$p.Dispose()}
}
function Check-Payload($manifest){
 $hashes=Get-Content "$b\$manifest" -Raw | ConvertFrom-Json
 foreach($p in $hashes.PSObject.Properties){
  $actual=(Get-FileHash (Join-Path $script:install $p.Name)).Hash.ToLower()
  Assert ($actual -eq $p.Value) "Payload differs: $($p.Name)"
 }
 return @($hashes.PSObject.Properties).Count
}
function Shared-FileHash($path){
 $stream=[IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete));$sha=[Security.Cryptography.SHA256]::Create()
 try{return [BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-','')}finally{$sha.Dispose();$stream.Dispose()}
}
$lock=$null
try {
 Assert ($env:USERNAME -eq 'OttrinIndexQA') 'Wrong account'
 $id=[Security.Principal.WindowsIdentity]::GetCurrent()
 Assert (-not (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Elevated actor'
 $install=Join-Path $env:LOCALAPPDATA 'Programs\Ottrin';$app=Join-Path $install 'bin\ottrin.exe'
 Assert (-not(Test-Path $install)) 'Unexpected installation'
 $pref=Join-Path $env:APPDATA 'ottrin\config.json'
 if(-not $ReuseOwnedProfile){Assert (-not(Test-Path $pref)) 'Unexpected existing preferences'}
 $root=Join-Path $env:USERPROFILE 'OttrinFixtures'
 if(-not $ReuseOwnedProfile){New-Item $root -ItemType Directory|Out-Null}
 $source=Join-Path $root 'scheduled-daemon-fixture.txt'
 if($ReuseOwnedProfile){
  Assert ([IO.File]::ReadAllText($source) -eq 'NSIS cleanup source fixture') 'Owned source differs'
  $config=Get-Content $pref -Raw|ConvertFrom-Json
  Assert ($config.search.include_roots.Count -eq 1 -and $config.search.include_roots[0] -eq $root) 'Owned preferences differ'
 }else{
  [IO.File]::WriteAllText($source,'NSIS cleanup source fixture')
  $config=Get-Content "$b\nsis-cleanup-config.json" -Raw|ConvertFrom-Json
  $config.search.include_roots=@($root);$config.search.refresh_interval_hours=0
  New-Item (Split-Path $pref) -ItemType Directory -Force|Out-Null
  [IO.File]::WriteAllText($pref,($config|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false))
 }
 $prefHash=(Get-FileHash $pref).Hash;$sourceHash=(Get-FileHash $source).Hash
 $result.preference_hash_before=$prefHash;$result.source_hash_before=$sourceHash
 $result.installer_sha256=(Get-FileHash "$b\nsis-current-installer.exe").Hash.ToLower()
 Assert ($result.installer_sha256 -eq $ExpectedInstallerSha256) 'Wrong installer'
 $p=Start-Process "$b\nsis-current-installer.exe" -ArgumentList '/S' -PassThru;$null=$p.Handle
 Assert ($p.WaitForExit(120000) -and $p.ExitCode -eq 0) 'Install failed';$result.install_exit=$p.ExitCode
 $result.payload_files=Check-Payload 'nsis-current-hashes.json'
 $result.update=Invoke-App 'index update --json'
 $result.query=(Invoke-App 'search scheduled-daemon-fixture --json')|ConvertFrom-Json
 Assert ($result.query.total -eq 1) 'Expected one fixture result'
 $p=Start-Process "$b\nsis-current-installer.exe" -ArgumentList '/S' -PassThru;$null=$p.Handle
 Assert ($p.WaitForExit(120000) -and $p.ExitCode -eq 0) 'Reinstall failed';$result.reinstall_exit=$p.ExitCode
 $null=Check-Payload 'nsis-current-hashes.json'
 $scheduler=New-Object -ComObject Schedule.Service;$scheduler.Connect();$folder=$scheduler.GetFolder('\')
 $name='Ottrin Index '+$id.User.Value;$before=$folder.GetTask($name)
 Assert ($before.GetInstances(0).Count -eq 1) 'Index not running'
 $beforeXml=$before.Xml;$instance=$before.GetInstances(0).Item(1).InstanceGuid
 $database=Join-Path $env:LOCALAPPDATA 'ottrin\search.db';$dbHash=Shared-FileHash $database
 $cache=Join-Path $env:LOCALAPPDATA 'ottrin\preview_cache';New-Item $cache -ItemType Directory -Force|Out-Null
 $file=Join-Path $cache 'nsis-locked-cache';[IO.File]::WriteAllText($file,'OWNED NSIS CACHE')
 $lock=[IO.File]::Open($file,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
 $result.phase='blocked-uninstall'
 $p=Start-Process "$install\Uninstall.exe" -ArgumentList "/S _?=$install" -PassThru;$null=$p.Handle
 Assert ($p.WaitForExit(120000)) 'Failure probe timeout'
 Assert ($p.ExitCode -eq 1) "Blocked cleanup should return 1: $($p.ExitCode)";$result.failed_uninstall_exit=$p.ExitCode
 $null=Check-Payload 'nsis-current-hashes.json'
 $after=$folder.GetTask($name)
 Assert ($after.Xml -eq $beforeXml -and $after.GetInstances(0).Count -eq 1) 'Service changed during failure'
 Assert ($after.GetInstances(0).Item(1).InstanceGuid -eq $instance) 'Daemon instance replaced'
 Assert ((Shared-FileHash $database) -eq $dbHash) 'Index bytes changed'
 Assert (Test-Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Ottrin') 'Installed Apps registration lost'
 $result.index_hash_after_failure=$dbHash;$result.same_live_instance_after_failure=$true;$result.registration_retained_after_failure=$true
 $lock.Dispose();$lock=$null
 $result.phase='uninstall-retry'
 $p=Start-Process "$install\Uninstall.exe" -ArgumentList '/S' -PassThru;$null=$p.Handle
 Assert ($p.WaitForExit(30000) -and $p.ExitCode -eq 0) 'Uninstall bootstrap failed';$result.uninstall_bootstrap_exit=$p.ExitCode
 $deadline=[DateTime]::UtcNow.AddSeconds(30)
 while((Test-Path $install) -and [DateTime]::UtcNow -lt $deadline){Start-Sleep -Milliseconds 200}
 Assert (-not(Test-Path $install)) 'App directory remains'
 Assert (-not(Test-Path $database)) 'Generated index remains'
 Assert (-not(Test-Path $file)) 'Owned cache file remains'
 if(Test-Path $cache){Assert (@(Get-ChildItem $cache).Count -eq 0) 'Cache entries remain'}
 $result.empty_cache_directory_retained=Test-Path $cache
 Assert (-not(Test-Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Ottrin')) 'Installed Apps key remains'
 Assert (-not(Test-Path (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Ottrin'))) 'Start menu remains'
 $absent=$false;try{$null=$folder.GetTask($name)}catch{if($_.Exception.GetBaseException().HResult -eq -2147024894){$absent=$true}else{throw}}
 Assert $absent 'Index task remains'
 Assert ((Get-FileHash $pref).Hash -eq $prefHash) 'Preferences changed'
 Assert ((Get-FileHash $source).Hash -eq $sourceHash) 'Source changed'
 $result.preference_hash_after=(Get-FileHash $pref).Hash;$result.source_hash_after=(Get-FileHash $source).Hash
 $result.task_removed=$true;$result.generated_data_removed=$true;$result.status='pass';$result.phase='uninstalled'
}catch{$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally{if($lock){$lock.Dispose()}}
$result|ConvertTo-Json -Depth 30|Set-Content "$r\result.json" -Encoding UTF8
if($result.status -ne 'pass'){exit 1};exit 0
