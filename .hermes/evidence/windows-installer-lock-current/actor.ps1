$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$r='C:\OttrinQA-20261011\installer-lock-current'
$mutex=$null;$active=@();$gate=$null;$results=@()
function Assert($condition,$message){if(-not $condition){throw $message}}
function Start-Owned($exe,$arguments){
 $p=Start-Process -FilePath $exe -ArgumentList $arguments -PassThru
 $null=$p.Handle;$script:active+=,$p;return $p
}
function Complete($p){
 Assert ($p.WaitForExit(30000)) "Timed out process $($p.Id)"
 $p.WaitForExit();Assert ($null -ne $p.ExitCode) 'Unavailable exit code';return $p.ExitCode
}
function Run-Owned($exe,$arguments){$p=Start-Owned $exe $arguments;return (Complete $p)}
function Hashes($folder){
 $v=@{};Get-ChildItem $folder -Recurse -File | ForEach-Object {$v[$_.FullName.Substring($folder.Length)]=(Get-FileHash $_.FullName).Hash};return ($v|ConvertTo-Json -Compress)
}
function Uninstaller($folder,$name){$runner="$folder\$name.exe";Copy-Item "$folder\install\Uninstall.exe" $runner;return $runner}
try {
 Assert (-not (Test-Path 'HKCU:\Software\OttrinInstallerLockQA')) 'Owned registration exists'
 Assert (-not (Test-Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\OttrinInstallerLockQA')) 'Owned uninstall registration exists'
 $menu=Join-Path ([Environment]::GetFolderPath('Programs')) 'OttrinInstallerLockQA'
 Assert (-not (Test-Path $menu)) 'Owned shortcuts exist'
 Assert (-not (Test-Path "$env:LOCALAPPDATA\ottrin-installer-lock-qa")) 'Owned user-data root exists'
 foreach($label in @('baseline','fixed','overlap')) {
  $case="$r\$label";New-Item -ItemType Directory $case | Out-Null
  [IO.File]::WriteAllText("$case\source.txt",'source preserved')
  [IO.File]::WriteAllText("$case\preferences.json",'{"preserve":true}')
  $setup=if($label -eq 'baseline'){"$r\baseline.exe"}else{"$r\fixed.exe"}
  Assert ((Run-Owned $setup "/S /D=$case\install") -eq 0) "$label install"
  Assert (Test-Path "$case\install\Uninstall.exe") "$label no uninstaller"
  $before=Hashes "$case\install";$runner=Uninstaller $case 'uninstall-runner'
  if($label -ne 'overlap') {
   $mutex=[Threading.Mutex]::new($false,'Global\OttrinInstaller')
   $code=Run-Owned $runner "/S _?=$case\install"
   if($label -eq 'baseline') {
    Assert ($code -eq 0) 'Baseline must demonstrate unguarded removal'
    Assert (Test-Path "$case\cleanup-attempted.txt") 'Baseline did not call cleanup'
    Assert (-not (Test-Path "$case\install")) 'Baseline did not remove payload'
    $results+=@{case=$label;held_lock_exit=$code;cleanup_called=$true;payload_removed=$true}
   } else {
    Assert ($code -eq 1) 'Fixed uninstaller did not refuse'
    Assert (-not (Test-Path "$case\cleanup-attempted.txt")) 'Fixed uninstaller called cleanup under held mutex'
    Assert ((Hashes "$case\install") -ceq $before) 'Fixed refusal changed payload'
    Assert (Test-Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\OttrinInstallerLockQA') 'Fixed refusal removed registration'
    Assert ((Run-Owned $setup "/S /D=$case\install") -eq 1) 'Fixed setup did not refuse'
    Assert ((Hashes "$case\install") -ceq $before) 'Setup refusal changed payload'
    $mutex.Dispose();$mutex=$null
    Assert ((Run-Owned $runner "/S _?=$case\install") -eq 0) 'Fixed retry failed'
    Assert (-not (Test-Path "$case\install")) 'Fixed retry left payload'
    $results+=@{case=$label;held_lock_exit=$code;cleanup_called_under_lock=$false;payload_and_registration_preserved_on_refusal=$true;setup_refuses_same_lock=$true;retry_exit=0}
   }
   if($null -ne $mutex){$mutex.Dispose();$mutex=$null}
  } else {
   $gate="$case\block-cleanup";[IO.File]::WriteAllText($gate,'owned gate')
   $first=Start-Owned $runner "/S _?=$case\install"
   $deadline=[DateTime]::UtcNow.AddSeconds(10)
   while(-not (Test-Path "$case\cleanup-attempted.txt")) {Assert ([DateTime]::UtcNow -lt $deadline) 'Cleanup did not start';Start-Sleep -Milliseconds 50}
   $other=Uninstaller $case 'second-uninstaller'
   Assert ((Run-Owned $other "/S _?=$case\install") -eq 1) 'Concurrent uninstaller not refused'
   Assert ((Run-Owned $setup "/S /D=$case\install") -eq 1) 'Setup racing uninstall not refused'
   Assert (@(Get-Content "$case\cleanup-attempted.txt").Count -eq 1) 'Second cleanup was invoked'
   Assert ((Hashes "$case\install") -ceq $before) 'Concurrent attempt changed payload'
   Remove-Item $gate;$gate=$null
   Assert ((Complete $first) -eq 0) 'First uninstall failed'
   Assert (-not (Test-Path "$case\install")) 'First uninstall left payload'
   $results+=@{case=$label;real_uninstaller_held_lock=$true;second_uninstaller_exit=1;concurrent_setup_exit=1;cleanup_calls=1;payload_preserved_until_first_cleanup_resumes=$true;first_uninstall_exit=0}
  }
  Assert ((Get-Content "$case\source.txt" -Raw) -ceq 'source preserved') 'Source changed'
  Assert ((Get-Content "$case\preferences.json" -Raw) -ceq '{"preserve":true}') 'Preferences changed'
  Assert (-not (Test-Path 'HKCU:\Software\OttrinInstallerLockQA')) 'Registration remains'
  Assert (-not (Test-Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\OttrinInstallerLockQA')) 'Uninstall registration remains'
  Assert (-not (Test-Path $menu)) 'Shortcuts remain'
  if(Test-Path "$env:LOCALAPPDATA\ottrin-installer-lock-qa"){Remove-Item "$env:LOCALAPPDATA\ottrin-installer-lock-qa" -Recurse -Force}
 }
 @{status='pass';cases=$results;identity=(& whoami);groups=(& whoami /groups|Out-String)} | ConvertTo-Json -Depth 6 | Set-Content "$r\RESULTS.json"
} catch {$_|Out-String|Set-Content "$r\failure.txt";exit 1}
finally {
 if($null -ne $mutex){$mutex.Dispose()}
 if($null -ne $gate -and (Test-Path $gate)){Remove-Item $gate}
 foreach($p in $active) {
  if(-not $p.HasExited){if(-not $p.WaitForExit(3000)){Stop-Process -Id $p.Id -Force}}
  $p.Dispose()
 }
}
