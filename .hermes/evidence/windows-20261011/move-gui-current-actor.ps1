param([ValidateSet('Launch','Move','Finish')][string]$Phase)
$ProgressPreference='SilentlyContinue';$ErrorActionPreference='Stop'
$b='C:\OttrinQA-20261011';$r="$b\move-gui-current";$result=@{phase=$Phase;status='running'}
$env:APPDATA="$r\roaming";$env:LOCALAPPDATA="$r\local";$env:OTTRIN_RENDERER='opengl';$env:OTTRIN_DEV_HELPER_AUTOBUILD='0'
$app="$b\move-gui-payload\Ottrin-0.1.0-windows-x64\bin\ottrin.exe"
function Assert($ok,$message){if(-not $ok){throw $message}}
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
Add-Type @'
using System;using System.Runtime.InteropServices;
public static class MoveQA {
 [StructLayout(LayoutKind.Sequential)] public struct RECT {public int Left,Top,Right,Bottom;}
 [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT rect);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern void mouse_event(uint f,uint x,uint y,uint d,UIntPtr extra);
}
'@
$null=[MoveQA]::SetProcessDPIAware()
function Capture($p,$name){
 $rect=[MoveQA+RECT]::new();Assert ([MoveQA]::GetWindowRect($p.MainWindowHandle,[ref]$rect)) 'Window geometry unavailable'
 $bitmap=[Drawing.Bitmap]::new($rect.Right-$rect.Left,$rect.Bottom-$rect.Top);$g=[Drawing.Graphics]::FromImage($bitmap)
 try{$g.CopyFromScreen($rect.Left,$rect.Top,0,0,$bitmap.Size);$bitmap.Save("$r\$name.png",[Drawing.Imaging.ImageFormat]::Png)}finally{$g.Dispose();$bitmap.Dispose()}
 return $rect
}
try {
 Assert ($env:USERNAME -eq 'OttrinIndexQA') 'Wrong account'
 $identity=[Security.Principal.WindowsIdentity]::GetCurrent();Assert (-not ([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Elevated actor'
 if($Phase -eq 'Launch'){
  New-Item "$r\roaming\ottrin","$r\local","$r\source" -ItemType Directory -Force|Out-Null
  $config=Get-Content "$b\move-gui-config.json" -Raw|ConvertFrom-Json
  $config.search.include_roots=@("$r\source",'R:\OttrinMoveCurrent');$config.search.refresh_interval_hours=0
  [IO.File]::WriteAllText("$r\roaming\ottrin\config.json",($config|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false))
  [IO.File]::WriteAllText("$r\source\payload.txt",'CURRENT PUBLIC CROSS VOLUME MOVE')
  $result.source_hash=(Get-FileHash "$r\source\payload.txt").Hash
  $result.source_volume=(Get-Volume -DriveLetter C).UniqueId;$result.destination_volume=(Get-Volume -DriveLetter R).UniqueId
  Assert ($result.source_volume -ne $result.destination_volume) 'Not separate volumes'
  $p=Start-Process $app -ArgumentList "--select $r\source\payload.txt" -PassThru -RedirectStandardOutput "$r\app.stdout" -RedirectStandardError "$r\app.stderr"
  $null=$p.Handle;$p.Id|Set-Content "$r\pid.txt"
  $deadline=[DateTime]::UtcNow.AddSeconds(40)
  do{$p.Refresh();Assert (-not $p.HasExited) 'App exited before window';if($p.MainWindowHandle -ne 0){break};Start-Sleep -Milliseconds 100}while([DateTime]::UtcNow -lt $deadline)
  Assert ($p.MainWindowHandle -ne 0) 'Window timeout';Start-Sleep -Seconds 5;$p.Refresh()
  $result.window=Capture $p 'before';$result.pid=$p.Id;$result.title=$p.MainWindowTitle
 }else{
  $p=Get-Process -Id ([int](Get-Content "$r\pid.txt"));$p.Refresh()
  if($Phase -eq 'Move'){
   $rect=Capture $p 'focused-before'
   $null=[MoveQA]::SetForegroundWindow($p.MainWindowHandle);Start-Sleep -Milliseconds 200
   $null=[MoveQA]::SetCursorPos($rect.Left+220,$rect.Top+16);[MoveQA]::mouse_event(2,0,0,0,[UIntPtr]::Zero);[MoveQA]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
   Start-Sleep -Milliseconds 200;Assert ([MoveQA]::GetForegroundWindow() -eq $p.MainWindowHandle) 'Wrong foreground window'
   [Windows.Forms.SendKeys]::SendWait('^x');Start-Sleep -Milliseconds 200
   [Windows.Forms.SendKeys]::SendWait('^l');Start-Sleep -Milliseconds 200
   Set-Clipboard 'R:\OttrinMoveCurrent';[Windows.Forms.SendKeys]::SendWait('^a');[Windows.Forms.SendKeys]::SendWait('^v');[Windows.Forms.SendKeys]::SendWait('{ENTER}');Start-Sleep -Seconds 1
   [Windows.Forms.SendKeys]::SendWait('^v');Start-Sleep -Seconds 10
   $result.window=Capture $p 'after'
  }else{
   $baseline=Get-Content "$r\Launch.json" -Raw|ConvertFrom-Json
   Assert (-not(Test-Path "$r\source\payload.txt")) 'Source remains'
   Assert ((Get-FileHash 'R:\OttrinMoveCurrent\payload.txt').Hash -eq $baseline.source_hash) 'Destination bytes differ'
   $result.source_removed=$true;$result.destination_hash=(Get-FileHash 'R:\OttrinMoveCurrent\payload.txt').Hash
   $result.modules=@($p.Modules|Where-Object ModuleName -in @('opengl32.dll','libgallium_wgl.dll')|Select-Object ModuleName,FileName)
   $null=$p.Handle;$null=$p.CloseMainWindow();Assert ($p.WaitForExit(15000)) 'GUI did not close';$result.gui_exit=$p.ExitCode;Assert ($p.ExitCode -eq 0) 'GUI exit nonzero'
   $start=[Diagnostics.ProcessStartInfo]::new($app,'--uninstall');$start.UseShellExecute=$false
   $cleanup=[Diagnostics.Process]::Start($start);Assert ($cleanup.WaitForExit(30000) -and $cleanup.ExitCode -eq 0) 'Owned profile cleanup failed'
  }
 }
 $result.status='pass'
}catch{$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
$result|ConvertTo-Json -Depth 15|Set-Content "$r\$Phase.json" -Encoding UTF8
if($result.status -ne 'pass'){exit 1};exit 0
