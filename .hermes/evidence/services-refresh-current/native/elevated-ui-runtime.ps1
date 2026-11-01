$ErrorActionPreference='Stop'
$root='C:\OttrinJobFixtureQA'
$phase=(Get-Content "$root\phase" -Raw).Trim()
Start-Transcript -Path "$root\results\$phase.transcript"|Out-Null
$identity=[Security.Principal.WindowsIdentity]::GetCurrent()
$owner=Get-Content "$root\INTERACTIVE_OWNER.json" -Raw|ConvertFrom-Json
if($identity.User.Value-ne$owner.sid){throw 'Wrong actor'}
$admin=([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if(($phase-eq'limited'-and$admin)-or($phase-eq'elevated'-and-not$admin)){throw 'Incorrect token phase'}
if(-not$admin-or$identity.Owner-eq$identity.User){throw 'Test requires elevated distinct owner token'}
$homeRoot=Join-Path $root "data\$phase"
New-Item -ItemType Directory $homeRoot|Out-Null
foreach($pair in @(@('TEMP','t'),@('TMP','t'),@('APPDATA','a'),@('LOCALAPPDATA','l'),@('USERPROFILE','h'),@('HOME','h'))){$path=Join-Path $homeRoot $pair[1];New-Item -ItemType Directory $path -Force|Out-Null;[Environment]::SetEnvironmentVariable($pair[0],$path,'Process')}
$p=$null
try {
 Add-Type -TypeDefinition @'
using System; using System.Text; using System.Runtime.InteropServices; using System.ComponentModel;
public static class TempAlias {
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern uint GetShortPathNameW(string path,StringBuilder output,uint size);
 public static string Short(string path) {var output=new StringBuilder(32768);uint size=GetShortPathNameW(path,output,(uint)output.Capacity);if(size==0||size>=output.Capacity)throw new Win32Exception(Marshal.GetLastWin32Error());return output.ToString();}
}
'@
 $original=$env:TEMP;$short=[TempAlias]::Short($original)
 @{original=$original;short=$short;scope='Only this owned actor and its fixture children use the existing NTFS short alias; no volume or system policy changed'}|ConvertTo-Json|Set-Content "$root\results\$phase-alias.json"
 if($short-ceq$original-or-not$short.Contains('~')){throw 'Existing NTFS short alias unavailable; no reproduction accepted'}
 $env:TEMP=$short;$env:TMP=$short
 $actorTempEntries=@(Get-ChildItem $env:TEMP -Force|ForEach-Object Name)
 $native=([char]0xd800).ToString();$arguments='"C:\unused-native-'+$native+'.txt" --help';$launch=Start-Process "$root\fixture-launch.exe" -ArgumentList $arguments -WorkingDirectory $homeRoot -PassThru -RedirectStandardOutput "$root\results\$phase-launch.stdout" -RedirectStandardError "$root\results\$phase-launch.stderr";$null=$launch.Handle;try{if(-not$launch.WaitForExit(10000)){throw 'Raw native argv help timed out'};if($launch.ExitCode-ne0){throw "Native argv launch failed $($launch.ExitCode)"};$help=Get-Content "$root\results\$phase-launch.stdout" -Raw;if(-not$help.Contains('Usage:')){throw 'Raw native argv did not reach CLI help'};@{exit=$launch.ExitCode;nativeUnit=[int][char]$native;helpReached=$true;scope='Actual application process,unpaired native UTF-16 argv,no GUI or index expected'}|ConvertTo-Json|Set-Content "$root\results\$phase-launch.json"}finally{if(-not$launch.HasExited){& C:\Windows\System32\taskkill.exe /PID $launch.Id /T /F|Out-Null;$null=$launch.WaitForExit(10000)}}
 $inventory=Start-Process "$root\fixture-ui.exe" -ArgumentList @('--list','refresh_keeps_actions_in_place_until_new_status_arrives') -WorkingDirectory $homeRoot -PassThru -RedirectStandardOutput "$root\results\$phase.list" -RedirectStandardError "$root\results\$phase-list.stderr";$null=$inventory.Handle;if(-not$inventory.WaitForExit(30000)){Stop-Process -Id $inventory.Id -Force;throw 'Inventory timed out'};if($inventory.ExitCode-ne0){throw "Test inventory failed $($inventory.ExitCode)"}
 $p=Start-Process "$root\fixture-ui.exe" -ArgumentList @('refresh_keeps_actions_in_place_until_new_status_arrives','--nocapture','--test-threads=1') -WorkingDirectory $homeRoot -PassThru -RedirectStandardOutput "$root\results\$phase.stdout" -RedirectStandardError "$root\results\$phase.stderr"
 $null=$p.Handle
 if(-not$p.WaitForExit(300000)){throw 'Native fixture timeout'}
 @{phase=$phase;exit=$p.ExitCode;user=$identity.User.Value;owner=$identity.Owner.Value;administrator=$admin;tempEntries=@(Get-ChildItem $env:TEMP -Force).Count;actorTempEntries=$actorTempEntries;uiRemainingEntries=@(Get-ChildItem $env:TEMP -Force|Where-Object {$_.Name -notin $actorTempEntries}|Select-Object Name,Attributes,Length);remainingTempEntries=@(Get-ChildItem $env:TEMP -Force|Select-Object -First 20 Name,Attributes,Length)}|ConvertTo-Json|Set-Content "$root\results\$phase.json"
 if($p.ExitCode-ne0){throw "UI suite failed $($p.ExitCode)"}
 $out=Get-Content "$root\results\$phase.stdout" -Raw
}finally{if($p-and-not$p.HasExited){& C:\Windows\System32\taskkill.exe /PID $p.Id /T /F|Out-Null;$null=$p.WaitForExit(10000)};Remove-Item -LiteralPath $homeRoot -Recurse -Force}
