$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$r='C:\OttrinQA-20261011\recovery-current'
foreach($key in @('HKCU:\Software\OttrinRecoveryQA','HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\OttrinRecoveryQA')){if(Test-Path $key){throw "Fixture integration remains: $key"}}
$menu=Join-Path ([Environment]::GetFolderPath('Programs')) 'OttrinRecoveryQA'
if(Test-Path $menu){throw 'Fixture shortcuts remain'}
if(Test-Path "$env:LOCALAPPDATA\ottrin-recovery-qa"){throw 'Fixture profile data remains'}
$live=@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath -like "$r\*" -or $_.CommandLine -like '*OTTRIN_INSTALLER_PAYLOAD_SCRIPT*'})
if($live.Count){throw 'Fixture installers or payload helpers remain'}
$hashes=@{};foreach($name in @('baseline.exe','fixed.exe','actor.ps1','payload-transaction.qa.ps1')){$hashes[$name]=(Get-FileHash "$r\$name").Hash.ToLowerInvariant()}
@{status='pass';artifacts=$hashes;fixture_registry_shortcuts_and_profile_absent=$true;live_fixture_installers_and_payload_helpers=0;identity=(& whoami)}|ConvertTo-Json -Depth 5|Set-Content "$r\USER_CLEANUP.json"
