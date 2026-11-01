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
 public static int Open(string directory,IntPtr klass){var i=new Info();i.size=(uint)Marshal.SizeOf(i);i.mask=0x40|0x100|0x400;i.verb="Ottrin.Open";i.file=directory;i.directory=directory;i.show=1;if(klass!=IntPtr.Zero){i.mask|=3;i.key=klass;}if(!ShellExecuteExW(ref i))throw new Win32Exception(Marshal.GetLastWin32Error());if(i.process==IntPtr.Zero)throw new Exception("Missing owned child handle");try{return (int)GetProcessId(i.process);}finally{CloseHandle(i.process);}}
}
'@
function Pump {[Windows.Forms.Application]::DoEvents();Start-Sleep -Milliseconds 40}
function Cli([string]$arguments) {
 $info=New-Object Diagnostics.ProcessStartInfo;$info.FileName="$root\fixture-launch.exe";$info.Arguments=$arguments;$info.UseShellExecute=$false;$info.CreateNoWindow=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true;$info.StandardOutputEncoding=[Text.UTF8Encoding]::new($false);$info.StandardErrorEncoding=[Text.UTF8Encoding]::new($false)
 $p=New-Object Diagnostics.Process;$p.StartInfo=$info
 try {if(-not$p.Start()){throw 'CLI did not start'};$out=$p.StandardOutput.ReadToEndAsync();$err=$p.StandardError.ReadToEndAsync();if(-not$p.WaitForExit(15000)){& C:\Windows\System32\taskkill.exe /PID $p.Id /T /F|Out-Null;$null=$p.WaitForExit(10000);throw 'CLI timed out'};if($p.ExitCode-ne0){throw ($arguments+': '+$err.Result)};return $out.Result} finally {$p.Dispose()}
}
function WaitWindow([int]$pidValue,[string]$title) {
 $deadline=[DateTime]::UtcNow.AddSeconds(25);$window=[IntPtr]::Zero
 do {Pump;$window=[PickerNative]::Window($pidValue,$title)}while($window-eq[IntPtr]::Zero-and[DateTime]::UtcNow-lt$deadline)
 if($window-eq[IntPtr]::Zero){throw ('Owned window missing: '+$title)};return $window
}
function DefaultSnapshot {
 $result=@{}
 foreach($path in @('Software\Classes\Directory\shell','Software\Classes\Directory\Background\shell','Software\Classes\Drive\shell')) {
  $key=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($path)
  try {$exists=$key-and(''-in$key.GetValueNames());$result[$path]=@{exists=[bool]$exists;kind=$(if($exists){$key.GetValueKind('').ToString()}else{''});value=$(if($exists){$key.GetValue('',$null,[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)}else{$null})}}finally{if($key){$key.Dispose()}}
 }
 return ($result|ConvertTo-Json -Depth 8 -Compress)
}
$intent=$false;$report=@{scope='Production per-user folder CLI,actual merged Shell verbs,source main/Services GUI,repair and uninstall; compatibility renderer on virtual graphics';windows=@()}
try {
 if(@(Get-CimInstance Win32_Process|Where-Object {$_.Name-ieq'ottrin-indexd.exe'}).Count){throw 'Existing index daemon retained; fixture requires a free user endpoint'}
 $before=Cli 'folders status --json'|ConvertFrom-Json;if($before.enabled){throw 'Existing integration retained; fixture requires an unconfigured account'}
 foreach($path in @('Directory\shell','Directory\Background\shell','Drive\shell')) {$key=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey(('Software\Classes\'+$path+'\Ottrin.Open'));if($key){$key.Dispose();throw 'Unmanaged existing action retained'}}
 $defaults=DefaultSnapshot;$corpus=Join-Path $homeRoot 'source%1 100% literal';New-Item -ItemType Directory $corpus|Out-Null;$source=Join-Path $corpus 'note.txt';[IO.File]::WriteAllText($source,'preserve folder integration');$hash=(Get-FileHash $source).Hash
 New-Item -ItemType Directory "$env:APPDATA\ottrin"|Out-Null
 $config=@{theme='Dark';default_view_mode='Miller';graphics_renderer='OpenGlCompatibility';graphics_compatibility_notice_seen=$true;restore_session=$false;last_session_dirs=@();search=@{include_roots=@($corpus);exclude_roots=@();refresh_interval_hours=0;content_indexing_enabled=$false;use_system_locate=$false;manage_locate_db=$false}}
 [IO.File]::WriteAllText("$env:APPDATA\ottrin\config.json",($config|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false))
 @{nonce=$nonce;originalUnconfigured=$true;expectedExecutable="$root\fixture-launch.exe"}|ConvertTo-Json|Set-Content "$root\results\folder-intent.json";$intent=$true
 $null=Cli 'folders enable';$status=Cli 'folders status --json'|ConvertFrom-Json;if(-not$status.enabled-or-not$status.ready){throw 'Production folder setup not ready'};$report.enabled=$status
 foreach($case in @('directory','background','drive')) {
  $target=if($case-eq'drive'){'C:\'}else{$corpus};$klass=$null
  try {if($case-eq'background'){$klass=[Microsoft.Win32.Registry]::ClassesRoot.OpenSubKey('Directory\Background');if(-not$klass){throw 'Merged background class absent'}};$pidValue=[FolderInvoke]::Open($target,$(if($klass){$klass.Handle.DangerousGetHandle()}else{[IntPtr]::Zero}))}finally{if($klass){$klass.Dispose()}}
  $window=WaitWindow $pidValue 'Ottrin';for($i=0;$i-lt15;$i++){Pump};[PickerNative]::Shot($window,"$root\results\$phase-$case.png")
  $process=Get-Process -Id $pidValue;$null=$process.Handle;$command=(Get-CimInstance Win32_Process -Filter "ProcessId=$pidValue").CommandLine
  if($case-eq'directory') {
   [PickerNative]::Click($window,-24,-24);$settings=WaitWindow $pidValue 'Ottrin Settings';for($i=0;$i-lt8;$i++){Pump};[PickerNative]::Wheel($settings,-1200);for($i=0;$i-lt12;$i++){Pump};[PickerNative]::Shot($settings,"$root\results\$phase-services.png")
   # Remove one exactly-owned entry to verify the real Repair action.
   $key=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\Ottrin\DesktopIntegration');try{$registration=[Text.Encoding]::UTF8.GetString($key.GetValue('FolderCommands'))|ConvertFrom-Json}finally{$key.Dispose()}
   $parent=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\Classes\Directory\Background\shell',$true);try{$entry=$parent.OpenSubKey('Ottrin.Open');try{if([Convert]::ToBase64String($entry.GetValue('OttrinOwner'))-ne[Convert]::ToBase64String([byte[]]$registration.owner)){throw 'Damage fixture ownership changed'}}finally{$entry.Dispose()};$parent.DeleteSubKeyTree('Ottrin.Open',$false)}finally{$parent.Dispose()}
   # Refresh, then repair through the card. The result is checked through the shared CLI.
   [PickerNative]::Click($settings,680,-48);for($i=0;$i-lt8;$i++){Pump};[PickerNative]::Click($settings,280,-48)
   $deadline=[DateTime]::UtcNow.AddSeconds(8);do {Pump;$fixed=Cli 'folders status --json'|ConvertFrom-Json}while(-not$fixed.ready-and[DateTime]::UtcNow-lt$deadline)
   $report.uiRepairVerified=[bool]$fixed.ready;[PickerNative]::Shot($settings,"$root\results\$phase-repair.png");if(-not$fixed.ready){throw 'Rendered Repair action did not restore missing owned entry'}
   [PickerNative]::Click($settings,-22,16);for($i=0;$i-lt5;$i++){Pump}
  }
  [PickerNative]::Click($window,-22,18);$deadline=[DateTime]::UtcNow.AddSeconds(15);while(-not$process.HasExited-and[DateTime]::UtcNow-lt$deadline){Pump};if(-not$process.HasExited){throw 'Main window did not close normally'};$process.WaitForExit();if($process.ExitCode-ne0){throw 'Main process failed'}
  $saved=Get-Content "$env:APPDATA\ottrin\config.json" -Raw|ConvertFrom-Json;$expected=$target.TrimEnd('\');$locations=@($saved.last_session_dirs|ForEach-Object {$_.Replace('\\?\','').TrimEnd('\')});if($expected-notin$locations){throw ('Shell arguments did not select requested folder: '+$command+' saved='+($locations-join','))}
  $report.windows+=@{case=$case;exit=$process.ExitCode;commandLine=$command;requestedDirectoryVerified=$true};$process.Dispose()
 }
 $null=Cli 'folders disable';$off=Cli 'folders status --json'|ConvertFrom-Json;if($off.enabled-or$off.ready){throw 'Folder setup not removed'};if((DefaultSnapshot)-ne$defaults){throw 'Existing folder defaults changed'};if((Get-FileHash $source).Hash-ne$hash){throw 'Source changed'}
 $report.status='pass';$report.off=$off;$report.defaultsPreserved=$true;$report.sourcePreserved=$true;$report|ConvertTo-Json -Depth 12|Set-Content "$root\results\$phase.json"
} catch {($_|Out-String)+$_.ScriptStackTrace|Set-Content "$root\results\$phase-error.txt";throw} finally {
 foreach($p in @(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath-ieq"$root\fixture-launch.exe"-or$_.ExecutablePath-ieq"$root\ottrin-indexd.exe"})){$owned=Get-Process -Id $p.ProcessId -ErrorAction SilentlyContinue;if($owned){$null=$owned.Handle;if($owned.MainModule.FileName-in@("$root\fixture-launch.exe","$root\ottrin-indexd.exe")){$owned.Kill();if(-not$owned.WaitForExit(10000)){throw 'Owned process failed to exit'}};$owned.Dispose()}}
 if($intent){$null=Cli 'folders disable'}
 Remove-Item -LiteralPath $homeRoot -Recurse -Force
}
