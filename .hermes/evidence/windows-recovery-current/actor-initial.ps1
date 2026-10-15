$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$r='C:\OttrinQA-20261011\recovery-current'
$active=@();$results=@()
function Assert($v,$m){if(-not $v){throw $m}}
function StartOwned($exe,$arg){$p=Start-Process -FilePath $exe -ArgumentList $arg -PassThru;$null=$p.Handle;$script:active+=,$p;return $p}
function Run($exe,$arg){$p=StartOwned $exe $arg;Assert ($p.WaitForExit(45000)) "Timed out $exe";$p.WaitForExit();return $p.ExitCode}
function RegistryDigest($keys){
 $ordered=[ordered]@{}
 foreach($key in $keys){foreach($v in $key.values){$ordered[$key.path+'\'+$v.name]=@{exists=$v.exists;kind=$v.kind;value=$v.value}}}
 # Compare named fields; Hashtable property order is not a semantic difference.
 return $ordered
}
function AssertRegistry($expected,$actual){
 $a=RegistryDigest $expected;$b=RegistryDigest $actual
 Assert ($a.Count -eq $b.Count) 'Registry count changed'
 foreach($name in $a.Keys){foreach($field in @('exists','kind','value')){Assert ((ConvertTo-Json -Compress -InputObject $a[$name][$field]) -ceq (ConvertTo-Json -Compress -InputObject $b[$name][$field])) "Registry mismatch $name $field"}}
}
function AssertLinks($expected,$actual,$folder){
 $shell=New-Object -ComObject WScript.Shell
 foreach($link in $expected){
  $other=@($actual|Where-Object name -eq $link.name)
  Assert ($other.Count -eq 1) 'Shortcut observation missing'
  if($null -eq $link.bytes){Assert ($null -eq $other[0].bytes) 'Absent shortcut recreated';continue}
  Assert ($null -ne $other[0].bytes) 'Original shortcut missing'
  $a=Join-Path $folder 'expected-shortcut.lnk';$b=Join-Path $folder 'actual-shortcut.lnk'
  [IO.File]::WriteAllBytes($a,[Convert]::FromBase64String($link.bytes));[IO.File]::WriteAllBytes($b,[Convert]::FromBase64String($other[0].bytes))
  $old=$shell.CreateShortcut($a);$now=$shell.CreateShortcut($b)
  foreach($property in @('TargetPath','Arguments','Description','IconLocation','WorkingDirectory','Hotkey','WindowStyle')){Assert ($old.$property -eq $now.$property) "Shortcut $($link.name) $property changed"}
  Remove-Item $a,$b
 }
}
function KillTree($p){
 if(-not $p.HasExited){& "$env:WINDIR\System32\taskkill.exe" /PID $p.Id /T /F|Out-Null;Assert ($LASTEXITCODE -eq 0) 'Cannot kill owned installer tree'}
 Assert ($p.WaitForExit(10000)) 'Interrupted installer remains';Start-Sleep -Milliseconds 250
}
$source=Get-Content "$r\payload-transaction.qa.ps1" -Raw
$definition=[regex]::Match($source,"(?s)Add-Type -TypeDefinition @'\r?\n(.*?)\r?\n'@").Groups[1].Value
Add-Type -TypeDefinition $definition
$cases=@('staging','publishing','old-moved','candidate-moved','published','integrating','rollback-candidate-moved','rollback-old-restored','rolled-back','committed','cleanup-leaf','cleanup-tree','fresh-published','fresh-committed','foreign-target','foreign-staged','torn-journal','missing-snapshot','root-extra','final-receipt','final-empty','foreign-completed')
try {
 Assert (-not (Test-Path 'HKCU:\Software\OttrinRecoveryQA')) 'Fixture integration exists'
 foreach($case in $cases){
  $folder=Join-Path $r ($case+" Space Ω 'copy'");New-Item -ItemType Directory $folder|Out-Null
  $target=Join-Path $folder 'install';$env:OTTRIN_QA_CONTROL_LOG=Join-Path $folder 'control.log'
  $env:OTTRIN_QA_FAIL_PHASE=$null;$env:OTTRIN_QA_PAUSE_AT=$null;$env:OTTRIN_QA_OBSERVATION=$null
  $fresh=$case.StartsWith('fresh-')
  if(-not $fresh){
   Assert ((Run "$r\baseline.exe" "/S /D=$target") -eq 0) 'Baseline failed'
   $metadata=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\Microsoft\Windows\CurrentVersion\Uninstall\OttrinRecoveryQA',$true)
   try{$metadata.SetValue('NoModify',[byte[]]@(0,255),[Microsoft.Win32.RegistryValueKind]::Binary);$metadata.SetValue('NoRepair',[long]4294967297,[Microsoft.Win32.RegistryValueKind]::QWord)}finally{$metadata.Dispose()}
   if($case -notin @('cleanup-tree','root-extra','final-receipt','final-empty','foreign-completed')){[IO.File]::WriteAllText("$target\unowned.txt",'keep user data')}
  }
  [IO.File]::WriteAllText("$folder\preferences.json",'{"preserved":true}')
  $point=$case -replace '^fresh-',''
  if($case -in @('foreign-target','torn-journal','missing-snapshot')){$point='published'}
  if($case -eq 'foreign-staged'){$point='staging'}
  if($case -eq 'root-extra'){$point='cleanup-tree'}
  if($case -eq 'foreign-completed'){$point='final-receipt'}
  if($point.StartsWith('rollback-') -or $point -eq 'rolled-back'){$env:OTTRIN_QA_FAIL_PHASE='Activate'}
  $env:OTTRIN_QA_PAUSE_AT=$point;$env:OTTRIN_QA_CHECKPOINT_FILE=Join-Path $folder 'checkpoint.json'
  $p=StartOwned "$r\fixed.exe" "/S /D=$target"
  $deadline=[DateTime]::UtcNow.AddSeconds(45)
  while(-not (Test-Path $env:OTTRIN_QA_CHECKPOINT_FILE)){
   Assert (-not $p.HasExited) "Installer exited before $case checkpoint"
   Assert ([DateTime]::UtcNow -lt $deadline) "No $case checkpoint";Start-Sleep -Milliseconds 100
  }
  $checkpoint=Get-Content $env:OTTRIN_QA_CHECKPOINT_FILE -Raw|ConvertFrom-Json
  Assert ($checkpoint.point -ceq $point) 'Wrong checkpoint'
  KillTree $p
  $env:OTTRIN_QA_PAUSE_AT=$null;$env:OTTRIN_QA_FAIL_PHASE=$null
  $terminal=$case -in @('final-receipt','final-empty','foreign-completed')
  $journalPath=if($terminal){"$target.ottrin-completed\transaction.json"}else{"$target.ottrin-setup\transaction.json"}
  $journalBytes=if(Test-Path $journalPath){[IO.File]::ReadAllBytes($journalPath)}else{$null}
  $journal=$checkpoint.transaction
  $committed=$journal.state -eq 'committed'
  if($case -eq 'root-extra'){[IO.File]::WriteAllText("$target.ottrin-setup\unexpected-note.txt",'retained root evidence')}
  if($case -in @('foreign-target','foreign-staged','torn-journal','missing-snapshot','foreign-completed')){
   switch($case){
    'foreign-target' {[IO.Directory]::Move($target,"$folder\held-target");New-Item -ItemType Directory $target|Out-Null;[IO.File]::WriteAllText("$target\foreign.txt",'foreign preserved')}
    'foreign-staged' {[IO.Directory]::Move("$target.ottrin-setup\new","$folder\held-staged");New-Item -ItemType Directory "$target.ottrin-setup\new"|Out-Null;[IO.File]::WriteAllText("$target.ottrin-setup\new\foreign.txt",'foreign preserved')}
    'foreign-completed' {[IO.File]::WriteAllText("$target.ottrin-completed\unexpected.txt",'do not consume')}
    'torn-journal' {[IO.File]::WriteAllText($journalPath,'{"torn":')}
    'missing-snapshot' {[IO.File]::Move("$target.ottrin-setup\index-transaction.json","$folder\held-snapshot.json")}
   }
   [IO.File]::WriteAllBytes($env:OTTRIN_QA_CONTROL_LOG,[byte[]]@())
   Assert ((Run "$r\fixed.exe" "/S /D=$target") -eq 1) 'Invalid recovery was accepted'
   Assert ([IO.File]::ReadAllBytes($env:OTTRIN_QA_CONTROL_LOG).Length -eq 0) 'Invalid recovery ran an index controller'
   switch($case){
    'foreign-target' {Assert ((Get-Content "$target\foreign.txt" -Raw) -ceq 'foreign preserved') 'Foreign target changed';Remove-Item "$target\foreign.txt";Remove-Item $target;[IO.Directory]::Move("$folder\held-target",$target)}
    'foreign-staged' {Assert ((Get-Content "$target.ottrin-setup\new\foreign.txt" -Raw) -ceq 'foreign preserved') 'Foreign staging changed';Remove-Item "$target.ottrin-setup\new\foreign.txt";Remove-Item "$target.ottrin-setup\new";[IO.Directory]::Move("$folder\held-staged","$target.ottrin-setup\new")}
    'foreign-completed' {Assert ((Get-Content "$target.ottrin-completed\unexpected.txt" -Raw) -ceq 'do not consume') 'Unknown completed data changed';Remove-Item "$target.ottrin-completed\unexpected.txt"}
    'torn-journal' {Assert ((Get-Content $journalPath -Raw) -ceq '{"torn":') 'Torn journal changed';[IO.File]::WriteAllBytes($journalPath,$journalBytes)}
    'missing-snapshot' {[IO.File]::Move("$folder\held-snapshot.json","$target.ottrin-setup\index-transaction.json")}
   }
  }
  $env:OTTRIN_QA_OBSERVATION=Join-Path $folder 'recovery-observation.json'
  [IO.File]::WriteAllBytes($env:OTTRIN_QA_CONTROL_LOG,[byte[]]@())
  Assert ((Run "$r\fixed.exe" "/S /D=$target") -eq 0) "$case retry failed"
  $observation=Get-Content $env:OTTRIN_QA_OBSERVATION -Raw|ConvertFrom-Json
  if($committed){Assert ($observation.live -ceq $journal.candidate) 'Committed root was rolled back';Assert ($observation.version -ceq 'new') 'Committed bytes rolled back'}
  else{
   Assert ($observation.live -ceq $journal.previous) 'Original root not restored before new setup'
   Assert ($observation.version -ceq $(if($fresh){$null}else{'old'})) 'Original bytes not restored'
   AssertRegistry $journal.registry $observation.registry
   AssertLinks $journal.links $observation.links $folder
  }
  $trace=[Text.Encoding]::Unicode.GetString([IO.File]::ReadAllBytes($env:OTTRIN_QA_CONTROL_LOG))
  $prepare=$trace.IndexOf('new --installer-transaction Prepare')
  $phase=if($committed){'Commit'}else{'Rollback'}
  $recovery=$trace.IndexOf('new --installer-transaction '+$phase)
  if($terminal){Assert ($prepare -ge 0 -and ($recovery -lt 0 -or $recovery -gt $prepare)) 'Terminal receipt cleanup ran task recovery'}
  else{Assert ($recovery -ge 0 -and $prepare -gt $recovery) 'Recovery did not precede new Prepare'}
  Assert (-not (Test-Path "$target.ottrin-completed")) 'Completed receipt not reclaimed'
  if($committed -or $journal.state -in @('staging','rolled-back')){Assert (-not $trace.Contains('new --stop')) 'Recovery stopped a committed or already restored daemon'}
  else{Assert ($trace.IndexOf('new --stop') -lt $recovery -and $trace.Contains('new --stop')) 'Published recovery did not stop candidate before rollback'}
  Assert (-not (Test-Path "$target.ottrin-setup")) 'Recovery/new setup left pending transaction'
  Assert ((Get-Content "$target\version.txt" -Raw) -ceq 'new') 'Retry did not publish new payload'
  Assert ((Get-Content "$folder\preferences.json" -Raw) -ceq '{"preserved":true}') 'Preferences changed'
  $retained=@(Get-ChildItem -LiteralPath $folder -Directory|Where-Object Name -like 'install.ottrin-retained-*')
  if(-not $fresh -and $case -notin @('cleanup-tree','root-extra','final-receipt','final-empty','foreign-completed')){
   Assert (@($retained|Where-Object {Test-Path "$($_.FullName)\previous\unowned.txt"}).Count -eq 1) 'Unowned old data not retained exactly once'
  }
  if($case -eq 'root-extra'){
   $extra=@($retained|Where-Object {Test-Path "$($_.FullName)\unexpected-note.txt"})
   Assert ($extra.Count -eq 1) 'Unknown root evidence lost'
   Assert (Test-Path "$($extra[0].FullName)\transaction.json") 'Journal lost before root retention'
  }
  $results+=@{case=$case;checkpoint=$point;durable_state=$journal.state;retry_exit=0;recovery_precedes_new_setup=$true;original_root_and_metadata_restored=(!$committed);committed_root_retained=$committed;index_phase_order_checked=$true;invalid_record_refused=($case -in @('foreign-target','foreign-staged','torn-journal','missing-snapshot','foreign-completed'));preferences_preserved=$true}
  $results|ConvertTo-Json -Depth 8|Set-Content "$r\PROGRESS.json"
  $env:OTTRIN_QA_OBSERVATION=$null
  Copy-Item "$target\Uninstall.exe" "$folder\uninstall-runner.exe"
  Assert ((Run "$folder\uninstall-runner.exe" "/S _?=$target") -eq 0) 'Final fixture uninstall failed'
  if(Test-Path "$env:LOCALAPPDATA\ottrin-recovery-qa"){Remove-Item "$env:LOCALAPPDATA\ottrin-recovery-qa" -Recurse -Force}
 }
 @{status='pass';cases=$results;identity=(& whoami);groups=(& whoami /groups|Out-String)}|ConvertTo-Json -Depth 8|Set-Content "$r\RESULTS.json"
}catch{$_|Out-String|Set-Content "$r\failure.txt";exit 1}
finally{
 $env:OTTRIN_QA_PAUSE_AT=$null;$env:OTTRIN_QA_FAIL_PHASE=$null;$env:OTTRIN_QA_OBSERVATION=$null
 foreach($p in $active){if(-not $p.HasExited){KillTree $p};$p.Dispose()}
}
