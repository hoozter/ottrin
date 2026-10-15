$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$r='C:\OttrinQA-20261011\payload-current'
$active=@();$hold=$null;$results=@()
function Assert($v,$m){if(-not $v){throw $m}}
function Run($exe,$arg){
 $p=Start-Process -FilePath $exe -ArgumentList $arg -PassThru;$null=$p.Handle;$script:active+=,$p
 Assert ($p.WaitForExit(45000)) "Timed out $exe";$p.WaitForExit();Assert ($null -ne $p.ExitCode) 'No exit code';return $p.ExitCode
}
function Hashes($path){
 $v=[ordered]@{};Get-ChildItem -LiteralPath $path -Recurse -File | Sort-Object FullName | ForEach-Object {$v[$_.FullName.Substring($path.Length)]=(Get-FileHash -LiteralPath $_.FullName).Hash};return ($v|ConvertTo-Json -Compress)
}
function RegistryState {
 $v=[ordered]@{}
 foreach($key in @('HKCU:\Software\OttrinPayloadQA','HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\OttrinPayloadQA')) {
  $item=Get-Item -LiteralPath $key
  foreach($name in ($item.GetValueNames()|Sort-Object)){$v[$key+'\'+$name]=@{kind=[int]$item.GetValueKind($name);value=$item.GetValue($name)}}
 }
 return ($v|ConvertTo-Json -Depth 4 -Compress)
}
# Borrow only the native identity type from the helper; do not execute a phase
# in the fixture process. This lets the fixture compare the old root's file ID.
$source=Get-Content "$r\payload-transaction.qa.ps1" -Raw
$definition=[regex]::Match($source,"(?s)Add-Type -TypeDefinition @'\r?\n(.*?)\r?\n'@").Groups[1].Value
Assert ($definition.Length -gt 100) 'Missing identity type';Add-Type -TypeDefinition $definition
Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;using Microsoft.Win32.SafeHandles;
public static class PayloadQALock {
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern SafeFileHandle CreateFile(string p,uint a,uint s,IntPtr sa,uint c,uint f,IntPtr t);
 public static SafeFileHandle Hold(string p){var h=CreateFile(p,0x10000,3,IntPtr.Zero,3,0x02000000,IntPtr.Zero);if(h.IsInvalid)throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());return h;}
}
'@
try {
 Assert (-not (Test-Path 'HKCU:\Software\OttrinPayloadQA')) 'Fixture registry already exists'
 Assert (-not (Test-Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\OttrinPayloadQA')) 'Fixture uninstall registry already exists'
 $foreign="$r\foreign-directory";New-Item -ItemType Directory $foreign|Out-Null
 [IO.File]::WriteAllText("$foreign\user-data.txt",'unrelated data')
 $foreignId=[OttrinInstallIdentity]::Read($foreign)
 Assert ((Run "$r\fixed.exe" "/S /D=$foreign") -eq 1) 'Foreign nonempty directory accepted'
 Assert ((Get-Content "$foreign\user-data.txt" -Raw) -ceq 'unrelated data') 'Foreign data changed'
 Assert ([OttrinInstallIdentity]::Read($foreign) -ceq $foreignId) 'Foreign root moved'
 Assert (-not (Test-Path "$foreign.ottrin-setup")) 'Foreign directory left a transaction'
 $results+=@{case='Foreign';exit=1;unrelated_directory_and_data_preserved=$true}
 foreach($case in @('Staging','Service','Commit','Lock','Stale','Success')) {
  $folder="$r\$case Space Ω 'copy'";New-Item -ItemType Directory $folder|Out-Null
  $target="$folder\install";$env:OTTRIN_QA_CONTROL_LOG="$folder\control.log";$env:OTTRIN_QA_FAIL_PHASE=$null
  [IO.File]::WriteAllText("$folder\source.txt",'source preserved');[IO.File]::WriteAllText("$folder\prefs.json",'{"preserved":true}')
  Assert ((Run "$r\baseline.exe" "/S /D=$target") -eq 0) 'Baseline installation failed'
  [IO.File]::WriteAllText("$target\unowned-user-file.txt",'unowned data preserved')
  $metadata=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\Microsoft\Windows\CurrentVersion\Uninstall\OttrinPayloadQA',$true)
  try {$metadata.SetValue('NoModify',[byte[]]@(0,255),[Microsoft.Win32.RegistryValueKind]::Binary);$metadata.SetValue('NoRepair',[long]4294967297,[Microsoft.Win32.RegistryValueKind]::QWord)} finally {$metadata.Dispose()}
  $unknown=[Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Software\OttrinPayloadQA\Custom')
  try {$unknown.SetValue('Preserve','unknown metadata')} finally {$unknown.Dispose()}
  $bytes=Hashes $target;$id=[OttrinInstallIdentity]::Read($target);$registry=RegistryState
  $menu=Join-Path ([Environment]::GetFolderPath('Programs')) 'OttrinPayloadQA'
  $linkBytes=Hashes $menu
  if($case -eq 'Lock'){$hold=[PayloadQALock]::Hold($target)}
  elseif($case -eq 'Stale') {New-Item -ItemType Directory "$target.ottrin-setup"|Out-Null;[IO.File]::WriteAllText("$target.ottrin-setup\unknown.txt",'do not consume')}
  elseif($case -ne 'Success'){$env:OTTRIN_QA_FAIL_PHASE=$case}
  $setup=if($case -eq 'Staging'){"$r\staging-fault.exe"}else{"$r\fixed.exe"}
  $exit=Run $setup "/S /D=$target"
  $trace=[Text.Encoding]::Unicode.GetString([IO.File]::ReadAllBytes("$folder\control.log"))
  if($case -in @('Service','Commit','Success')) {Assert ($trace.Contains('new index service-install')) "$case never reached new service setup"}
  if($case -eq 'Commit') {Assert ($trace.Contains('new --installer-transaction Commit')) 'Commit failure was not exercised'}
  if($case -in @('Service','Commit')) {Assert ($trace.Contains('new --stop')) 'New daemon was not stopped before rollback'}
  $env:OTTRIN_QA_FAIL_PHASE=$null
  if($null -ne $hold){$hold.Dispose();$hold=$null}
  if($case -eq 'Success') {
   Assert ($exit -eq 0) 'Successful update failed'
   Assert ((Get-Content "$target\version.txt" -Raw) -ceq 'new') 'New payload not published'
   Assert ([OttrinInstallIdentity]::Read($target) -cne $id) 'Old root still live'
   $retained=@(Get-ChildItem -LiteralPath $folder -Directory | Where-Object Name -like 'install.ottrin-retained-*')
   Assert ($retained.Count -eq 1) 'Unowned data retention missing'
   Assert ((Get-Content "$($retained[0].FullName)\previous\unowned-user-file.txt" -Raw) -ceq 'unowned data preserved') 'Unowned data lost'
   Assert (@(Get-ChildItem "$($retained[0].FullName)\previous" -Recurse -File).Count -eq 1) 'Superseded managed payload retained'
  } else {
   Assert ($exit -eq 1) "$case update did not fail"
   Assert ((Hashes $target) -ceq $bytes) "$case original payload bytes changed"
   Assert ([OttrinInstallIdentity]::Read($target) -ceq $id) "$case original directory identity changed"
   if($case -eq 'Staging') {Assert ((Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\OttrinPayloadQA').DisplayName -ceq 'changed while staging') 'Pre-integration metadata change was overwritten'}
   else {Assert ((RegistryState) -ceq $registry) "$case registration not restored"}
   Assert ((Hashes $menu) -ceq $linkBytes) "$case shortcuts not restored"
   if($case -eq 'Stale'){
    Assert ((Get-Content "$target.ottrin-setup\unknown.txt" -Raw) -ceq 'do not consume') 'Stale transaction changed'
    Remove-Item "$target.ottrin-setup\unknown.txt";Remove-Item "$target.ottrin-setup"
   } else {Assert (-not (Test-Path "$target.ottrin-setup")) "$case transaction residue"}
  }
  Assert ((Get-ItemProperty 'HKCU:\Software\OttrinPayloadQA\Custom').Preserve -ceq 'unknown metadata') 'Unknown registry data changed'
  Assert ((Get-Content "$folder\source.txt" -Raw) -ceq 'source preserved') 'Source changed'
  Assert ((Get-Content "$folder\prefs.json" -Raw) -ceq '{"preserved":true}') 'Preferences changed'
  $results+=@{case=$case;exit=$exit;old_root_identity=$id;payload_checked=$true;registry_shortcuts_checked=($case -ne 'Success');source_preferences_preserved=$true;control_log=$trace}
  $results|ConvertTo-Json -Depth 7|Set-Content "$r\PROGRESS.json"
  Copy-Item "$target\Uninstall.exe" "$folder\uninstall-runner.exe"
  if($case -eq 'Success') {
   New-Item -ItemType Directory "$target.ottrin-setup"|Out-Null
   [IO.File]::WriteAllText("$target.ottrin-setup\pending.txt",'pending evidence')
   $installed=Hashes $target;$beforeLog=[IO.File]::ReadAllBytes("$folder\control.log").Length
   Assert ((Run "$folder\uninstall-runner.exe" "/S _?=$target") -eq 1) 'Uninstall consumed pending installation'
   Assert ((Hashes $target) -ceq $installed) 'Pending refusal changed installed payload'
   Assert ([IO.File]::ReadAllBytes("$folder\control.log").Length -eq $beforeLog) 'Pending uninstall invoked application cleanup'
   Assert ((Get-Content "$target.ottrin-setup\pending.txt" -Raw) -ceq 'pending evidence') 'Pending evidence changed'
   Remove-Item "$target.ottrin-setup\pending.txt";Remove-Item "$target.ottrin-setup"
   $results+=@{case='PendingUninstall';exit=1;installed_payload_and_pending_evidence_preserved=$true;application_cleanup_not_called=$true}
  }
  Assert ((Run "$folder\uninstall-runner.exe" "/S _?=$target") -eq 0) 'Final fixture uninstall failed'
  if(Test-Path "$env:LOCALAPPDATA\ottrin-payload-qa"){Remove-Item "$env:LOCALAPPDATA\ottrin-payload-qa" -Recurse -Force}
 }
 @{status='pass';cases=$results;identity=(& whoami);groups=(& whoami /groups|Out-String)}|ConvertTo-Json -Depth 7|Set-Content "$r\RESULTS.json"
} catch {$_|Out-String|Set-Content "$r\failure.txt";exit 1}
finally {
 $env:OTTRIN_QA_FAIL_PHASE=$null;$env:OTTRIN_QA_CONTROL_LOG=$null
 if($null -ne $hold){$hold.Dispose()}
 foreach($p in $active){if(-not $p.HasExited){Stop-Process -Id $p.Id -Force};$p.Dispose()}
}
