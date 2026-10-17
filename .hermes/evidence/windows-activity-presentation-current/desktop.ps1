$ErrorActionPreference='Stop'
$root='C:\OttrinCopyCleanupQA\desktop'
trap { $_.ToString()|Set-Content -Encoding UTF8 "$root\results\actor-error.txt";exit 1 }
$identity=[Security.Principal.WindowsIdentity]::GetCurrent()
if(([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Desktop actor must remain ordinary'}
if($env:USERNAME-ne'OttrinCopyQA'-or[Diagnostics.Process]::GetCurrentProcess().SessionId-le0){throw 'Wrong owned interactive account/session'}
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
Add-Type @'
using System;using System.Runtime.InteropServices;
public static class CopyActivityDesktop {
 [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
 [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X,Y; }
 [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern void mouse_event(uint f,uint x,uint y,uint d,UIntPtr e);
 [DllImport("kernel32.dll")] public static extern uint WTSGetActiveConsoleSessionId();
}
'@
if([Diagnostics.Process]::GetCurrentProcess().SessionId-ne[CopyActivityDesktop]::WTSGetActiveConsoleSessionId()){throw 'Owned account is not the console session'}
$null=[CopyActivityDesktop]::SetProcessDPIAware()
$c=Get-Content "$root\command.json" -Raw|ConvertFrom-Json
if($c.case-notin@('success','changed','cohort')){throw 'Unknown owned fixture case'}
$caseRoot="$root\cases\$($c.case)";$app="$root\payload\Ottrin-0.1.0-windows-x64\bin\ottrin.exe"
$env:LOCALAPPDATA="$caseRoot\profile";$env:APPDATA="$caseRoot\roaming";$env:USERPROFILE="$caseRoot\userprofile"
$env:OTTRIN_TEST_WINDOWS_COPY_ACTIVITY_ROOT=$caseRoot
$env:OTTRIN_TEST_WINDOWS_COPY_ACTIVITY_CASE=if($c.case-eq'cohort'){'cleanup-cohort'}else{'cleanup-success'}
$processes=@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-eq$app})
if($processes.Count-gt1){throw 'Multiple owned GUI processes'}
if($c.op-eq'prepare'){
 if($processes.Count){throw 'Close owned GUI before fixture preparation'}
 if(Test-Path $caseRoot){throw 'Fixture already exists; preserve it for inspection'}
 New-Item -ItemType Directory $caseRoot|Out-Null;New-Item -ItemType Directory $env:USERPROFILE|Out-Null
 & C:\OttrinCopyCleanupQA\platform-fixture.exe --exact copy_transaction::windows::journal::tests::windows_copy_activity_fixture --ignored --nocapture *> "$root\results\fixture-$($c.case).log"
 if($LASTEXITCODE-ne0){throw 'Native owned fixture exporter failed'}
}elseif($c.op-eq'configure'){
 if($processes.Count){throw 'Close owned GUI before fixture configuration'}
 if($c.theme-notin@('Light','Dark')-or$c.text_scale-lt0.75-or$c.text_scale-gt2){throw 'Unsupported owned theme fixture'}
 $path="$caseRoot\roaming\ottrin\config.json";$config=Get-Content $path -Raw|ConvertFrom-Json
 $config.theme=$c.theme
 if($c.text_scale-ne1){
  $theme=(Get-Content "$root\theme-fixture.json" -Raw|ConvertFrom-Json).custom_themes[0]
  $theme.name='Activity QA';$theme.light.font_scale=[double]$c.text_scale;$theme.dark.font_scale=[double]$c.text_scale
  $config.custom_themes=@($theme);$config.active_custom_theme=$theme.name;$config.theme_preset=$theme.base_preset
 }else{$config.active_custom_theme=$null;$config.theme_preset='Ottrin'}
 [IO.File]::WriteAllText($path, ($config|ConvertTo-Json -Depth 100), [Text.UTF8Encoding]::new($false))
}elseif($c.op-eq'verify'){
 & "$root\verify.ps1"
}elseif($c.op-eq'launch'){
 if($processes.Count){throw 'Owned GUI already running'}
 if(-not(Test-Path "$caseRoot\fixture\destination")){throw 'Owned fixture missing'}
 $arguments=@("$caseRoot\fixture\destination");if($c.renderer-eq'opengl'){$arguments+=@('--renderer','opengl')}elseif($c.renderer-and$c.renderer-ne'auto'){throw 'Unknown renderer'}
 $p=Start-Process $app -ArgumentList $arguments -WorkingDirectory (Split-Path (Split-Path $app)) -PassThru -RedirectStandardOutput "$root\results\app-$($c.case).stdout" -RedirectStandardError "$root\results\app-$($c.case).stderr"
 $p.Id|Set-Content "$root\results\app-$($c.case).pid"
 Start-Sleep -Milliseconds 800
 }elseif($c.op-eq'stop'){
 foreach($owned in $processes){$p=Get-Process -Id $owned.ProcessId;if(-not$p.CloseMainWindow()){throw 'Owned app refused orderly close'};if(-not$p.WaitForExit(10000)){throw 'Owned app remains live'}}
 $daemons=@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-eq"$root\payload\Ottrin-0.1.0-windows-x64\bin\ottrin-indexd.exe"})
 foreach($owned in $daemons){Stop-Process -Id $owned.ProcessId -ErrorAction Stop}
 if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like"$root\payload\*"}).Count){throw 'Owned payload processes remain'}
}elseif($c.op-eq'cleanup'){
 if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like"$root\payload\*"}).Count){throw 'Owned payload must exit before cleanup'}
 Remove-Item -LiteralPath "$root\cases" -Recurse -Force
 if(Test-Path "$root\cases"){throw 'Owned fixtures remain'}
 @{status='pass';account=$env:USERNAME;cases_removed=$true;payload_processes=0}|ConvertTo-Json|Set-Content -Encoding UTF8 "$root\results\cleanup.json"
}elseif($c.op-eq'focus'){
 if($processes.Count-ne1){throw 'Owned GUI missing'}
 $point=New-Object CopyActivityDesktop+POINT;$point.X=[int]$c.x;$point.Y=[int]$c.y
 [uint32]$pointPid=0;$null=[CopyActivityDesktop]::GetWindowThreadProcessId([CopyActivityDesktop]::WindowFromPoint($point),[ref]$pointPid)
 if($pointPid-ne$processes[0].ProcessId){throw 'Visible focus point does not belong to the owned app'}
 $null=[CopyActivityDesktop]::SetCursorPos($point.X,$point.Y);[CopyActivityDesktop]::mouse_event(2,0,0,0,[UIntPtr]::Zero);[CopyActivityDesktop]::mouse_event(4,0,0,0,[UIntPtr]::Zero);Start-Sleep -Milliseconds 250
}elseif($c.op-eq'dismiss-notice'){
 [uint32]$foregroundPid=0;$null=[CopyActivityDesktop]::GetWindowThreadProcessId([CopyActivityDesktop]::GetForegroundWindow(),[ref]$foregroundPid)
 $foreground=Get-CimInstance Win32_Process|Where-Object{$_.ProcessId-eq$foregroundPid}
 if($foregroundPid-ne$c.foreground_pid-or$foreground.ExecutablePath-ine"$env:windir\SystemApps\ShellExperienceHost_cw5n1h2txyewy\ShellExperienceHost.exe"){throw 'Owned notification surface changed'}
 $point=New-Object CopyActivityDesktop+POINT;$point.X=[int]$c.x;$point.Y=[int]$c.y
 [uint32]$pointPid=0;$null=[CopyActivityDesktop]::GetWindowThreadProcessId([CopyActivityDesktop]::WindowFromPoint($point),[ref]$pointPid)
 if($pointPid-ne$foregroundPid){throw 'Dismiss point does not belong to the observed notification host'}
 $null=[CopyActivityDesktop]::SetCursorPos($point.X,$point.Y);[CopyActivityDesktop]::mouse_event(2,0,0,0,[UIntPtr]::Zero);[CopyActivityDesktop]::mouse_event(4,0,0,0,[UIntPtr]::Zero);Start-Sleep -Milliseconds 700
}elseif($c.op-eq'onboard'){
 [uint32]$foregroundPid=0;$null=[CopyActivityDesktop]::GetWindowThreadProcessId([CopyActivityDesktop]::GetForegroundWindow(),[ref]$foregroundPid)
 $foreground=Get-CimInstance Win32_Process|Where-Object{$_.ProcessId-eq$foregroundPid}
 if($foregroundPid-ne$c.foreground_pid-or$foreground.ExecutablePath-ine"$env:windir\system32\wwahost.exe"){throw 'Owned Windows onboarding surface changed'}
 $screen=[Windows.Forms.SystemInformation]::VirtualScreen;if(-not$screen.Contains([int]$c.x,[int]$c.y)){throw 'Onboarding point outside owned console'}
 $null=[CopyActivityDesktop]::SetCursorPos([int]$c.x,[int]$c.y);[CopyActivityDesktop]::mouse_event(2,0,0,0,[UIntPtr]::Zero);[CopyActivityDesktop]::mouse_event(4,0,0,0,[UIntPtr]::Zero);Start-Sleep -Milliseconds 700
}elseif($c.op-in@('click','key','paste')){
 if($processes.Count-ne1){throw 'Owned GUI is not running'}
 $p=Get-Process -Id $processes[0].ProcessId;$p.Refresh();$window=$p.MainWindowHandle
 if($window-eq[IntPtr]::Zero){throw 'Owned app window not ready'}
 $null=[CopyActivityDesktop]::SetForegroundWindow($window);Start-Sleep -Milliseconds 100
 [uint32]$foregroundPid=0;$null=[CopyActivityDesktop]::GetWindowThreadProcessId([CopyActivityDesktop]::GetForegroundWindow(),[ref]$foregroundPid)
 if($foregroundPid-ne$p.Id){throw 'Owned app did not receive foreground focus'}
 $rect=New-Object CopyActivityDesktop+RECT;$null=[CopyActivityDesktop]::GetWindowRect($window,[ref]$rect)
 if($c.op-eq'click'){
  if($c.x-lt$rect.Left-or$c.x-ge$rect.Right-or$c.y-lt$rect.Top-or$c.y-ge$rect.Bottom){throw 'Point outside owned app window'}
  $null=[CopyActivityDesktop]::SetCursorPos([int]$c.x,[int]$c.y);[CopyActivityDesktop]::mouse_event(2,0,0,0,[UIntPtr]::Zero);[CopyActivityDesktop]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
 }elseif($c.op-eq'key'){
  if($c.key-notin@('{ESC}','^l','^j','{ENTER}','{F5}','{TAB}')){throw 'Key not in owned QA input set'}
  [Windows.Forms.SendKeys]::SendWait($c.key)
 }else{[Windows.Forms.Clipboard]::SetText([string]$c.text);[Windows.Forms.SendKeys]::SendWait('^v')}
 Start-Sleep -Milliseconds 200
}elseif($c.op-ne'observe'){throw 'Unknown owned desktop operation'}
$screen=[Windows.Forms.SystemInformation]::VirtualScreen
$bitmap=New-Object Drawing.Bitmap($screen.Width,$screen.Height);$g=[Drawing.Graphics]::FromImage($bitmap)
try{$g.CopyFromScreen($screen.Left,$screen.Top,0,0,$bitmap.Size);$bitmap.Save("$root\results\desktop.png",[Drawing.Imaging.ImageFormat]::Png)}finally{$g.Dispose();$bitmap.Dispose()}
[uint32]$foregroundPid=0;$null=[CopyActivityDesktop]::GetWindowThreadProcessId([CopyActivityDesktop]::GetForegroundWindow(),[ref]$foregroundPid)
@{foreground=@(Get-CimInstance Win32_Process|Where-Object{$_.ProcessId-eq$foregroundPid}|Select-Object ProcessId,Name,ExecutablePath);account=$env:USERNAME;session=[Diagnostics.Process]::GetCurrentProcess().SessionId;operation=$c.op;case=$c.case;processes=@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like"$root\payload\*"}|Select-Object ProcessId,Name,ExecutablePath)}|ConvertTo-Json -Depth 5|Set-Content -Encoding UTF8 "$root\results\desktop.json"
