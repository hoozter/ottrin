$ErrorActionPreference='Stop';$root='C:\OttrinJobFixtureQA';$phase=(Get-Content "$root\phase" -Raw).Trim();$nonce=(Get-Content "$root\OWNER.json" -Raw|ConvertFrom-Json).nonce
Start-Transcript -Path "$root\results\$phase.transcript"|Out-Null
$identity=[Security.Principal.WindowsIdentity]::GetCurrent();if($identity.User.Value-ne'S-1-5-21-3183210791-1673105385-3262580894-1001'-or([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Expected ordinary test actor'}
$homeRoot=Join-Path $root "data\$phase";New-Item -ItemType Directory $homeRoot|Out-Null
foreach($pair in @(@('TEMP','t'),@('TMP','t'),@('APPDATA','a'),@('LOCALAPPDATA','l'),@('USERPROFILE','h'),@('HOME','h'))){$path=Join-Path $homeRoot $pair[1];New-Item -ItemType Directory $path -Force|Out-Null;[Environment]::SetEnvironmentVariable($pair[0],$path,'Process')}
. "$root\native-controls.ps1";[PickerNative]::Init()
Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;using System.ComponentModel;
public static class FolderInvoke {
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)]public struct Info {public uint size,mask;public IntPtr hwnd;public string verb,file,parameters,directory;public int show;public IntPtr instance,idList;public string klass;public IntPtr key;public uint hotkey;public IntPtr icon,process;}
 [DllImport("shell32.dll",CharSet=CharSet.Unicode,SetLastError=true)]static extern bool ShellExecuteExW(ref Info value);
 [DllImport("kernel32.dll")]static extern uint GetProcessId(IntPtr process);
 [DllImport("kernel32.dll")]static extern bool CloseHandle(IntPtr handle);
 public static int Open(string directory,IntPtr klass){var i=new Info();i.size=(uint)Marshal.SizeOf(i);i.mask=0x40|0x100|0x400;i.verb=null;i.file=directory;i.directory=directory;i.show=1;if(klass!=IntPtr.Zero){i.mask|=3;i.key=klass;}if(!ShellExecuteExW(ref i))throw new Win32Exception(Marshal.GetLastWin32Error());if(i.process==IntPtr.Zero)throw new Exception("Missing owned child handle");try{return (int)GetProcessId(i.process);}finally{CloseHandle(i.process);}}
}
'@
function Pump {[Windows.Forms.Application]::DoEvents();Start-Sleep -Milliseconds 40}
function Invoke-OwnedOttrin([string]$arguments) {
 $info=New-Object Diagnostics.ProcessStartInfo;$info.FileName="$root\fixture-launch.exe";$info.Arguments=$arguments;$info.UseShellExecute=$false;$info.CreateNoWindow=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true;$info.StandardOutputEncoding=[Text.UTF8Encoding]::new($false);$info.StandardErrorEncoding=[Text.UTF8Encoding]::new($false)
 $p=New-Object Diagnostics.Process;$p.StartInfo=$info
 try {if(-not$p.Start()){throw 'CLI did not start'};$out=$p.StandardOutput.ReadToEndAsync();$err=$p.StandardError.ReadToEndAsync();if(-not$p.WaitForExit(15000)){& C:\Windows\System32\taskkill.exe /PID $p.Id /T /F|Out-Null;$null=$p.WaitForExit(10000);throw 'CLI timed out'};if($p.ExitCode-ne0){throw ($arguments+': '+$err.Result)};return $out.Result} finally {$p.Dispose()}
}
function WaitWindow([int]$pidValue,[string]$title) {
 $deadline=[DateTime]::UtcNow.AddSeconds(25);$window=[IntPtr]::Zero
 do {Pump;$window=[PickerNative]::Window($pidValue,$title)}while($window-eq[IntPtr]::Zero-and[DateTime]::UtcNow-lt$deadline)
 if($window-eq[IntPtr]::Zero){throw ('Owned window missing: '+$title)};[PickerNative]::Place($window);return $window
}
function DefaultSnapshot {
 $result=@{}
 foreach($path in @('Software\Classes\Directory\shell','Software\Classes\Directory\Background\shell','Software\Classes\Drive\shell')) {
  $key=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($path)
  try {$exists=$key-and(''-in$key.GetValueNames());$result[$path]=@{exists=[bool]$exists;kind=$(if($exists){$key.GetValueKind('').ToString()}else{''});value=$(if($exists){$key.GetValue('',$null,[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)}else{$null})}}finally{if($key){$key.Dispose()}}
 }
 return ($result|ConvertTo-Json -Depth 8 -Compress)
}

$intent=$false;$report=@{scope='Source Windows default folder/drive ShellExecute,real Services enable/restore,standalone Properties and exact baseline restoration';windows=@()};$main=$null;$settings=[IntPtr]::Zero
try {
 if(@(Get-CimInstance Win32_Process|Where-Object {$_.Name-ieq'ottrin-indexd.exe'}).Count){throw 'Existing daemon retained; fixture needs a free endpoint'}
 $before=Invoke-OwnedOttrin 'folders status --json'|ConvertFrom-Json;if($before.enabled){throw 'Existing folder setup retained'}
 $defaults=DefaultSnapshot;$corpus=Join-Path $homeRoot 'source%1 100% literal';New-Item -ItemType Directory $corpus|Out-Null;$source=Join-Path $corpus 'note.txt';[IO.File]::WriteAllText($source,'preserve default folder fixture');$hash=(Get-FileHash $source).Hash
 New-Item -ItemType Directory "$env:APPDATA\ottrin"|Out-Null
 $config=@{theme='Dark';default_view_mode='Miller';graphics_renderer='OpenGlCompatibility';graphics_compatibility_notice_seen=$true;search=@{include_roots=@($corpus);exclude_roots=@();refresh_interval_hours=0;content_indexing_enabled=$false;use_system_locate=$false;manage_locate_db=$false}}
 [IO.File]::WriteAllText("$env:APPDATA\ottrin\config.json",($config|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false))
 @{nonce=$nonce;originalUnconfigured=$true;expectedExecutable="$root\fixture-launch.exe"}|ConvertTo-Json|Set-Content "$root\results\folder-intent.json";$intent=$true
 $null=Invoke-OwnedOttrin 'folders default-enable';$cliOn=Invoke-OwnedOttrin 'folders status --json'|ConvertFrom-Json;if(-not$cliOn.default_ready){throw 'CLI default opening is not ready'}
 if(@(Get-CimInstance Win32_Process|Where-Object {$_.Name-ieq'ottrin-indexd.exe'}).Count){throw 'Headless default setup started an index'}
 $null=Invoke-OwnedOttrin 'folders enable';$cliRepair=Invoke-OwnedOttrin 'folders status --json'|ConvertFrom-Json;if(-not$cliRepair.default_ready){throw 'Repair reset the default choice'}
 $null=Invoke-OwnedOttrin 'folders default-disable';$cliRestored=Invoke-OwnedOttrin 'folders status --json'|ConvertFrom-Json;if($cliRestored.default_enabled-or-not$cliRestored.enabled-or(DefaultSnapshot)-ne$defaults){throw 'CLI default restoration failed'}
 $null=Invoke-OwnedOttrin 'folders disable';$report.headlessDefaultSetupNoIndex=$true;$report.repairPreservedDefault=$true;$report.cliDefaultsRestored=$true
 $main=Start-Process "$root\fixture-launch.exe" -ArgumentList @('--',('"'+$corpus+'"')) -PassThru;$null=$main.Handle;$mainWindow=WaitWindow $main.Id 'Ottrin'
 [PickerNative]::Click($mainWindow,-24,-24);$settings=WaitWindow $main.Id 'Ottrin Settings';for($i=0;$i-lt12;$i++){Pump};[PickerNative]::Wheel($settings,-1200);for($i=0;$i-lt12;$i++){Pump};[PickerNative]::Shot($settings,"$root\results\$phase-before.png")
 $row=[PickerNative]::BottomButtonRow($settings);$report.enableRow=$row;[PickerNative]::Click($settings,280,$row)
 $end=[DateTime]::UtcNow.AddSeconds(10);do {Pump;$status=Invoke-OwnedOttrin 'folders status --json'|ConvertFrom-Json}while(-not$status.default_ready-and[DateTime]::UtcNow-lt$end)
 if(-not$status.ready-or-not$status.default_ready-or-not$status.default_enabled){throw ('Visible default action did not enable setup: '+($status|ConvertTo-Json -Compress))};$report.enabled=$status
 for($i=0;$i-lt15;$i++){Pump};[PickerNative]::Wheel($settings,-1200);for($i=0;$i-lt8;$i++){Pump};[PickerNative]::Shot($settings,"$root\results\$phase-enabled.png")
 foreach($case in @('directory','drive')) {
  $target=if($case-eq'drive'){'C:\'}else{$corpus};$pidValue=[FolderInvoke]::Open($target,[IntPtr]::Zero);$process=Get-Process -Id $pidValue;$null=$process.Handle
  try {
   if($process.MainModule.FileName-ine"$root\fixture-launch.exe"){throw 'Shell used a different application; it is left untouched'}
   $window=WaitWindow $pidValue 'Ottrin';for($i=0;$i-lt12;$i++){Pump};[PickerNative]::Shot($window,"$root\results\$phase-$case.png")
   $null=[PickerNative]::SetForegroundWindow($window);[Windows.Forms.SendKeys]::SendWait('^l');for($i=0;$i-lt8;$i++){Pump};[PickerNative]::Shot($window,"$root\results\$phase-$case-address.png");[Windows.Forms.SendKeys]::SendWait('{ESC}');for($i=0;$i-lt4;$i++){Pump}
   $command=(Get-CimInstance Win32_Process -Filter "ProcessId=$pidValue").CommandLine;[PickerNative]::Click($window,-22,18);$end=[DateTime]::UtcNow.AddSeconds(15);while(-not$process.HasExited-and[DateTime]::UtcNow-lt$end){Pump};if(-not$process.HasExited){throw 'Default-launched window did not close'};$process.WaitForExit();if($process.ExitCode-ne0){throw 'Default-launched process failed'}
   $report.windows+=@{case=$case;verb=$null;requestedDirectory=$target;commandLine=$command;exit=$process.ExitCode;actualLocation='Captured address requires review'}
  }finally{$process.Dispose()}
 }
 $property=Start-Process "$root\fixture-launch.exe" -ArgumentList @('--properties','--',('"'+$source+'"')) -PassThru;$null=$property.Handle
 try {
  $title='Ottrin '+[char]0x2014+' Properties '+[char]0x2014+' note.txt';$window=WaitWindow $property.Id $title;for($i=0;$i-lt15;$i++){Pump};[PickerNative]::Shot($window,"$root\results\$phase-properties.png");$null=[PickerNative]::SetForegroundWindow($window);[Windows.Forms.SendKeys]::SendWait('{ESC}');$end=[DateTime]::UtcNow.AddSeconds(15);while(-not$property.HasExited-and[DateTime]::UtcNow-lt$end){Pump};if(-not$property.HasExited){throw 'Properties did not close'};$property.WaitForExit();if($property.ExitCode-ne0){throw 'Properties failed'};$report.properties=@{exit=$property.ExitCode;standalone=$true;sourceUnchanged=((Get-FileHash $source).Hash-eq$hash)}
 }finally{$property.Dispose()}
 $null=[PickerNative]::SetForegroundWindow($settings);[PickerNative]::Wheel($settings,-1200);for($i=0;$i-lt10;$i++){Pump};$row=[PickerNative]::BottomButtonRow($settings);$report.restoreRow=$row;[PickerNative]::Click($settings,280,$row)
 $end=[DateTime]::UtcNow.AddSeconds(10);do {Pump;$restored=Invoke-OwnedOttrin 'folders status --json'|ConvertFrom-Json}while($restored.default_enabled-and[DateTime]::UtcNow-lt$end)
 if($restored.default_enabled-or-not$restored.enabled){throw 'Visible Restore did not retain menus and turn default opening off'};if((DefaultSnapshot)-ne$defaults){throw 'Original typed defaults were not restored'}
 for($i=0;$i-lt12;$i++){Pump};[PickerNative]::Shot($settings,"$root\results\$phase-restored.png");[PickerNative]::Click($settings,-22,16);for($i=0;$i-lt5;$i++){Pump};[PickerNative]::Click($mainWindow,-22,18);$end=[DateTime]::UtcNow.AddSeconds(15);while(-not$main.HasExited-and[DateTime]::UtcNow-lt$end){Pump};if(-not$main.HasExited){throw 'Initial main window did not close'};$main.WaitForExit();if($main.ExitCode-ne0){throw 'Initial main process failed'}
 $null=Invoke-OwnedOttrin 'folders disable';$off=Invoke-OwnedOttrin 'folders status --json'|ConvertFrom-Json;if($off.enabled-or$off.default_enabled-or$off.ready-or$off.default_ready){throw 'Folder setup remains'};if((DefaultSnapshot)-ne$defaults-or(Get-FileHash $source).Hash-ne$hash){throw 'Baseline or source changed'}
 $report.status='pass';$report.originalDefaultsPreserved=$true;$report.sourcePreserved=$true;$report.restored=$restored;$report.off=$off;$report|ConvertTo-Json -Depth 12|Set-Content "$root\results\$phase.json"
} catch {($_|Out-String)+$_.ScriptStackTrace|Set-Content "$root\results\$phase-error.txt";throw} finally {
 foreach($p in @(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath-ieq"$root\fixture-launch.exe"-or$_.ExecutablePath-ieq"$root\ottrin-indexd.exe"})){$owned=Get-Process -Id $p.ProcessId -ErrorAction SilentlyContinue;if($owned){$null=$owned.Handle;if($owned.MainModule.FileName-in@("$root\fixture-launch.exe","$root\ottrin-indexd.exe")){$owned.Kill();if(-not$owned.WaitForExit(10000)){throw 'Owned process failed to exit'}};$owned.Dispose()}}
 if($main){$main.Dispose()};if($intent){$null=Invoke-OwnedOttrin 'folders disable';if((DefaultSnapshot)-ne$defaults){throw 'Baseline defaults remain changed; retain fixture'}}
 Remove-Item -LiteralPath $homeRoot -Recurse -Force
}
