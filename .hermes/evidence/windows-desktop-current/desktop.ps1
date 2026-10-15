$ErrorActionPreference='Stop';Add-Type -AssemblyName System.Windows.Forms,System.Drawing
if($env:USERNAME-ne'OttrinDesktopQA' -or [Diagnostics.Process]::GetCurrentProcess().SessionId-le0){throw 'Wrong interactive surface'}
Add-Type @'
using System;using System.Runtime.InteropServices;
public static class DesktopInputQA {
 [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern void mouse_event(uint f,uint x,uint y,uint d,UIntPtr e);
}
'@
$null=[DesktopInputQA]::SetProcessDPIAware();$screen=[Windows.Forms.SystemInformation]::VirtualScreen
$c=Get-Content C:\OttrinDesktopQA\desktop-command.json -Raw|ConvertFrom-Json
if($c.op-eq'click'){
 if(-not$screen.Contains([int]$c.x,[int]$c.y)){throw 'Point outside owned desktop'}
 $null=[DesktopInputQA]::SetCursorPos($c.x,$c.y);[DesktopInputQA]::mouse_event(2,0,0,0,[UIntPtr]::Zero);[DesktopInputQA]::mouse_event(4,0,0,0,[UIntPtr]::Zero);Start-Sleep -Seconds 1
}elseif($c.op-ne'observe'){throw 'Unknown operation'}
$bitmap=New-Object Drawing.Bitmap($screen.Width,$screen.Height);$g=[Drawing.Graphics]::FromImage($bitmap)
try{$g.CopyFromScreen($screen.Left,$screen.Top,0,0,$bitmap.Size);$bitmap.Save('C:\OttrinDesktopQA\results\desktop.png',[Drawing.Imaging.ImageFormat]::Png)}finally{$g.Dispose();$bitmap.Dispose()}
