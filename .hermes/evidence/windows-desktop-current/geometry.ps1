$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';Add-Type -AssemblyName System.Windows.Forms,System.Drawing
if($env:USERNAME-ne'OttrinDesktopQA'){throw 'Wrong account'}
Add-Type @'
using System;using System.Runtime.InteropServices;
public static class GeometryQA {
 [StructLayout(LayoutKind.Sequential)] public struct RECT {public int Left,Top,Right,Bottom;}
 [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
}
'@
$null=[GeometryQA]::SetProcessDPIAware();$b='C:\OttrinDesktopQA';$app=(Get-Content "$b\source-probe-app.txt" -Raw).Trim();$p=$null;$result=@{status='running';scope='Source-built Windows main executable over current166 MSI runtime; standard user, 1280x800 primary desktop; not a newly packaged build'}
try {
 $env:OTTRIN_RENDERER='opengl';$env:OTTRIN_DEV_HELPER_AUTOBUILD='0'
 $p=Start-Process $app -ArgumentList "`"$env:USERPROFILE\OttrinFixtures`"" -PassThru;$null=$p.Handle
 $deadline=[DateTime]::UtcNow.AddSeconds(40)
 do{$p.Refresh();if($p.HasExited){throw 'Source GUI exited early'};if($p.MainWindowHandle-ne0 -and $p.MainWindowTitle-eq'Ottrin'){break};Start-Sleep -Milliseconds 100}while([DateTime]::UtcNow-lt$deadline)
 if($p.MainWindowHandle-eq0 -or $p.MainWindowTitle-ne'Ottrin'){throw 'Real main window timeout'};Start-Sleep -Seconds 3;$p.Refresh()
 $rect=New-Object GeometryQA+RECT;if(-not[GeometryQA]::GetWindowRect($p.MainWindowHandle,[ref]$rect)){throw 'Window geometry unavailable'}
 $area=[Windows.Forms.Screen]::FromHandle($p.MainWindowHandle).WorkingArea
 $window=[Drawing.Rectangle]::FromLTRB($rect.Left,$rect.Top,$rect.Right,$rect.Bottom)
 $result.window=@{left=$rect.Left;top=$rect.Top;right=$rect.Right;bottom=$rect.Bottom};$result.work_area=@{left=$area.Left;top=$area.Top;right=$area.Right;bottom=$area.Bottom};$result.executable_sha256=(Get-FileHash $app).Hash.ToLowerInvariant()
 $null=[GeometryQA]::SetForegroundWindow($p.MainWindowHandle);Start-Sleep -Milliseconds 300
 $bitmap=New-Object Drawing.Bitmap($window.Width,$window.Height);$g=[Drawing.Graphics]::FromImage($bitmap)
 try{$g.CopyFromScreen($window.Left,$window.Top,0,0,$bitmap.Size);$bitmap.Save("$b\results\geometry.png",[Drawing.Imaging.ImageFormat]::Png)}finally{$g.Dispose();$bitmap.Dispose()}
 if(-not$area.Contains($window)){throw 'Initial main window lies outside the work area'}
 if(-not$p.CloseMainWindow()){throw 'Normal close rejected'};if(-not$p.WaitForExit(15000)-or$p.ExitCode-ne0){throw 'GUI did not exit normally'};$result.normal_gui_exit=$true;$result.status='pass'
}catch{$result.status='fail';$result.error=$_.Exception.Message}
finally{
 if($p){if(-not$p.HasExited){$p.Kill();$null=$p.WaitForExit(5000)};$p.Dispose()}
 $cleanup=Start-Process $app -ArgumentList '--uninstall' -PassThru -WindowStyle Hidden;$null=$cleanup.Handle
 if(-not$cleanup.WaitForExit(45000)){throw "Owned cleanup remains live $($cleanup.Id)"};$result.cleanup_exit=$cleanup.ExitCode;$cleanup.Dispose()
 $result|ConvertTo-Json -Depth 8|Set-Content "$b\results\geometry-result.json"
}
if($result.status-ne'pass'-or$result.cleanup_exit-ne0){exit 1}
