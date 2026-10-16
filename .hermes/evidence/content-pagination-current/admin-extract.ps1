$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$root='C:\OttrinContentQA'
$installer=New-Object -ComObject WindowsInstaller.Installer
$db=$installer.OpenDatabase("$root\current.msi",0)
$view=$db.OpenView('SELECT `Action` FROM `AdminExecuteSequence`')
$view.Execute();$actions=@();while($record=$view.Fetch()){$actions+=$record.StringData(1)};$view.Close()
$allowed=@('CostInitialize','FileCost','CostFinalize','InstallValidate','InstallInitialize','InstallAdminPackage','InstallFiles','InstallFinalize','LaunchConditions')
foreach($action in $actions){if($action -notin $allowed){throw "Unexpected administrative action: $action"}}
$target="$root\admin-extract"
$p=Start-Process msiexec.exe -ArgumentList "/a $root\current.msi /qn /norestart TARGETDIR=$target /l*v $root\admin-extract.log" -PassThru
$null=$p.Handle
if(-not $p.WaitForExit(120000)){throw "Owned administrative extraction still running: $($p.Id)"}
if($p.ExitCode -ne 0){throw "Administrative extraction failed: $($p.ExitCode)"}
$main=@(Get-ChildItem $target -Filter ottrin.exe -Recurse)
if($main.Count -ne 1){throw "Expected exactly one main payload, got $($main.Count)"}
$payload=Split-Path (Split-Path $main[0].FullName)
$hashes=@{}
Get-ChildItem $payload -Recurse -File | ForEach-Object {$rel=$_.FullName.Substring($payload.Length+1).Replace('\','/');$hashes[$rel]=(Get-FileHash $_.FullName).Hash.ToLowerInvariant()}
$b=Get-Content "$root\baseline.json" -Raw|ConvertFrom-Json
if((Get-FileHash 'C:\Users\campbell\AppData\Roaming\ottrin\config.json').Hash -ne $b.preference_hash){throw 'User preference changed'}
if((Get-FileHash 'C:\Users\campbell\AppData\Local\Programs\Ottrin\Uninstall.exe').Hash -ne $b.prior_uninstaller_hash){throw 'Prior user installation changed'}
@{status='pass';msi_sha256=(Get-FileHash "$root\current.msi").Hash.ToLowerInvariant();admin_sequence=$actions;payload_sha256=$hashes;scope='Native administrative extraction and payload hashes only; no per-user install, UI or rollback claim';existing_profile_and_installation='unchanged'} | ConvertTo-Json -Depth 8 | Set-Content "$root\admin-extract-result.json"
