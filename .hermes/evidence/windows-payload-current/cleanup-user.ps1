$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$r='C:\OttrinQA-20261011\payload-current'
foreach($key in @('HKCU:\Software\OttrinPayloadQA','HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\OttrinPayloadQA')) {if(Test-Path $key){throw "Fixture integration remains: $key"}}
$menu=Join-Path ([Environment]::GetFolderPath('Programs')) 'OttrinPayloadQA'
if(Test-Path $menu){throw 'Fixture shortcuts remain'}
if(Test-Path "$env:LOCALAPPDATA\ottrin-payload-qa"){throw 'Fixture profile data remains'}
$live=@(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -like "$r\*" -or $_.CommandLine -like '*OTTRIN_INSTALLER_PAYLOAD_SCRIPT*' })
if($live.Count){throw 'Fixture installer or payload controller remains live'}
$hashes=@{};foreach($name in @('baseline.exe','fixed.exe','staging-fault.exe','payload-transaction.qa.ps1')){$hashes[$name]=(Get-FileHash "$r\$name").Hash.ToLowerInvariant()}
@{status='pass';source_artifacts=$hashes;fixture_registry_absent=$true;fixture_shortcuts_absent=$true;fixture_profile_data_absent=$true;live_fixture_installers_or_payload_controllers=0;identity=(& whoami)}|ConvertTo-Json -Depth 4|Set-Content "$r\USER_CLEANUP.json"
