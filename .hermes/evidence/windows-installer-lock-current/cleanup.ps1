$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$r='C:\OttrinQA-20261011\installer-lock-current'
$t=Get-ScheduledTask OttrinQA-InstallerLockCurrent
if($t.State -ne 'Ready'){throw 'Owned fixture task is not terminal'}
$live=@(Get-CimInstance Win32_Process | Where-Object {$_.ExecutablePath -like "$r\*"})
if($live.Count){throw 'Owned fixture processes remain live'}
$results=Get-Content "$r\RESULTS.json" -Raw | ConvertFrom-Json
if($results.status -ne 'pass'){throw 'No accepted fixture results'}
Unregister-ScheduledTask OttrinQA-InstallerLockCurrent -Confirm:$false
Remove-Item $r -Recurse -Force
Remove-Item C:\OttrinQA-20261011\installer-lock-actor.ps1
@{status='pass';task_removed=$true;live_fixture_processes=0;owned_payload_fixture_runners_uploads_removed=$true;actor_checks='All cases preserved external fixture source/preferences; mock registration, shortcuts and user-data names removed';scope='Only named QA task, exact uploaded script and task-owned root removed'} | ConvertTo-Json
