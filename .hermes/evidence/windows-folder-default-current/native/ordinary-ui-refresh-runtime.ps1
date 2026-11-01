$ErrorActionPreference='Stop'
$root='C:\OttrinJobFixtureQA'
$phase=(Get-Content "$root\phase" -Raw).Trim()
Start-Transcript -Path "$root\results\$phase.transcript"|Out-Null
$identity=[Security.Principal.WindowsIdentity]::GetCurrent()
$owner=Get-Content "$root\INTERACTIVE_OWNER.json" -Raw|ConvertFrom-Json
if($identity.User.Value-ne$owner.sid){throw 'Wrong actor'}
$admin=([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if(($phase-eq'limited'-and$admin)-or($phase-eq'elevated'-and-not$admin)){throw 'Incorrect token phase'}
if($admin){throw 'Interactive test requires ordinary token'}
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
 $p=Start-Process "$root\fixture-ui.exe" -ArgumentList @('tests::folder_refresh_keeps_actions_in_place_until_new_status_arrives','--nocapture','--test-threads=1') -WorkingDirectory $homeRoot -PassThru -RedirectStandardOutput "$root\results\$phase.stdout" -RedirectStandardError "$root\results\$phase.stderr"
 $null=$p.Handle
 if(-not$p.WaitForExit(300000)){throw 'Native fixture timeout'}
 @{phase=$phase;exit=$p.ExitCode;user=$identity.User.Value;owner=$identity.Owner.Value;administrator=$admin;tempEntries=@(Get-ChildItem $env:TEMP -Force).Count;actorTempEntries=$actorTempEntries;uiRemainingEntries=@(Get-ChildItem $env:TEMP -Force|Where-Object {$_.Name -notin $actorTempEntries}|Select-Object Name,Attributes,Length);remainingTempEntries=@(Get-ChildItem $env:TEMP -Force|Select-Object -First 20 Name,Attributes,Length)}|ConvertTo-Json|Set-Content "$root\results\$phase.json"
 if($p.ExitCode-ne0){throw "UI suite failed $($p.ExitCode)"}
 $out=Get-Content "$root\results\$phase.stdout" -Raw
}finally{if($p-and-not$p.HasExited){& C:\Windows\System32\taskkill.exe /PID $p.Id /T /F|Out-Null;$null=$p.WaitForExit(10000)};Remove-Item -LiteralPath $homeRoot -Recurse -Force}
