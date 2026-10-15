$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$b='C:\OttrinQA-20261011'
$t=Get-ScheduledTask OttrinQA-OfficeLoopCurrent
if($t.State -ne 'Ready'){throw 'Probe task is not terminal'}
$executables=@('office-loop-pump.exe','office-loop-control.exe','office-loop-control-wct.exe')
$processes=@(Get-CimInstance Win32_Process|Where-Object {$executables -contains $_.Name})
if($processes.Count){throw 'Owned native probes remain live'}
$identities=@{}
foreach($name in $executables){$identities[$name]=(Get-FileHash "$b\$name").Hash.ToLowerInvariant()}
$original=Get-ChildItem "$b\office-formats" -File -Force | ForEach-Object {@{name=$_.Name;length=$_.Length;sha256=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}}
Unregister-ScheduledTask OttrinQA-OfficeLoopCurrent -Confirm:$false
foreach($name in @('office-loop-current','office-loop-current2','office-loop-current3')){Remove-Item "$b\$name" -Recurse -Force}
foreach($name in ($executables+@('office-loop-probe.ps1','office-loop-probe-wct.ps1','office-loop-probe-pump-fresh.ps1'))){Remove-Item "$b\$name" -Force}
if(Get-ScheduledTask OttrinQA-OfficeLoopCurrent -ErrorAction SilentlyContinue){throw 'task remains'}
@{status='pass';owned_task_removed=$true;owned_probe_processes=0;private_fixture_roots_removed=$true;executable_sha256=$identities;original_corpus_retained=$original;scope='Only exact named probe task, probe executables/scripts and three task-owned directories removed; preexisting corpus, lock, installation and user profile preserved'}|ConvertTo-Json -Depth 5
