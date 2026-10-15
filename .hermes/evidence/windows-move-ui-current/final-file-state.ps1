$ErrorActionPreference='Stop';$b='C:\OttrinMoveUIQA';$r="$b\results"
function Assert($v,$m){if(-not$v){throw $m}}
function Decode($record){Assert ($record.encoding-eq'windows-utf16-le') 'Native path encoding';$s='';for($i=0;$i-lt$record.units_hex.Length;$i+=4){$s+=[char][Convert]::ToUInt16($record.units_hex.Substring($i,4),16)};$s}
function Id($path){$v=[regex]::Match((& fsutil file queryfileid $path|Out-String),'0x[0-9a-fA-F]+').Value;Assert ($v.Length-gt10) 'Missing file identity';$v}
try{
 Assert ($env:USERNAME-eq'OttrinMoveUIQA') 'Wrong actor'
 $same=Get-Content "$r\SAME_PASS_CROSS_FIXTURE_FAILURE.json" -Raw|ConvertFrom-Json;$cross=Get-Content "$r\RESULTS.json" -Raw|ConvertFrom-Json
 Assert (@($same.cases).Count-eq5-and$cross.status-eq'pass'-and@($cross.cases).Count-eq5) 'Expected five accepted cases per volume'
 $checks=@()
 foreach($case in @($same.cases)+@($cross.cases)){
  $run=if($case.name.StartsWith('same-')){'MoveQA6'}else{'MoveQA8'};$source="$env:USERPROFILE\$run\$($case.name)\s";$dest=if($run-eq'MoveQA6'){"$env:USERPROFILE\$run\$($case.name)\d"}else{"R:\$run\$($case.name)\d"};$items=@(Get-ChildItem $source -Force)
  if($case.source_removed){Assert ($items.Count-eq0) 'Source claim or payload remains after completed move'}else{Assert ($items.Count-eq1-and$items[0].Name-ceq'payload.txt'-and[IO.File]::ReadAllText($items[0].FullName)-ceq("source:"+$case.name)-and(Id $items[0].FullName)-ceq$case.source_id) 'Skip/Cancel source changed later'}
  $target=Join-Path $dest $(if($case.renamed){$case.renamed}else{'payload.txt'});$expected=if($case.source_removed){'source:'+$case.name}else{'destination:'+$case.name};Assert ([IO.File]::ReadAllText($target)-ceq$expected) 'Final destination differs'
  if($case.renamed -or -not $case.source_removed){$oldTarget=Join-Path $dest 'payload.txt';Assert ([IO.File]::ReadAllText($oldTarget)-ceq('destination:'+$case.name)-and(Id $oldTarget)-ceq$case.destination_original_id) 'Original destination changed later'}
  $pair=$null
  if($case.retained){$central=$case.retained.journal;$record=Get-Content $central -Raw|ConvertFrom-Json;Assert ($record.phase-eq'published') 'Retained transaction not published';$stage=Decode $record.transaction_directory;$local=@(Get-ChildItem $stage -File -Force|Where-Object Name -like '*journal*');Assert ($local.Count-eq1-and[IO.File]::ReadAllText($local[0].FullName).Trim()-ceq[IO.File]::ReadAllText($central).Trim()) 'Journal pair differs';$retained=Decode $record.retained_displaced;Assert ([IO.File]::ReadAllText($retained)-ceq('destination:'+$case.name)-and(Id $retained)-ceq$case.destination_original_id) 'Retained displaced identity/data changed';$pair=@{central_sha256=(Get-FileHash $central).Hash;local_sha256=(Get-FileHash $local[0].FullName).Hash;retained_id=(Id $retained)}}
  else{Assert (@(Get-ChildItem $dest -Force|Where-Object Name -like '.ottrin-move-*').Count-eq0) 'Non-overwrite stage remains'}
  $checks+=@{name=$case.name;source_parent_entries=$items.Count;final_destination_verified=$true;pair=$pair}
 }
 Assert (-not(Test-Path "$env:LOCALAPPDATA\Programs\Ottrin")) 'Normal uninstall left payload'
 $scheduler=New-Object -ComObject Schedule.Service;$scheduler.Connect();$taskName='Ottrin Index '+[Security.Principal.WindowsIdentity]::GetCurrent().User.Value;$absent=$false;try{$null=$scheduler.GetFolder('\').GetTask($taskName)}catch{if($_.Exception.GetBaseException().HResult-eq-2147024894){$absent=$true}else{throw}};Assert $absent 'Index task remains after normal uninstall'
 @{status='pass';index_task_absent=$true;cases=$checks;scope='Post-GUI-close and normal-uninstall bytes/identities, empty moved-source parents, no non-overwrite stages, and exact paired published overwrite journals'}|ConvertTo-Json -Depth 12|Set-Content "$r\FINAL_FILE_STATE.json"
}catch{@{status='fail';error=$_.Exception.Message;stack=$_.ScriptStackTrace}|ConvertTo-Json|Set-Content "$r\FINAL_FILE_STATE.json";exit 1}
