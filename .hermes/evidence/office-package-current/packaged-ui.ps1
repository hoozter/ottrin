$ErrorActionPreference='Stop'
$r='C:\OttrinCopyCleanupQA\desktop';$bin="$r\payload\Ottrin-0.1.0-windows-x64\bin";$owned=@();$report=@{status='running';cases=@{}}
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;using System.Text;using System.Drawing;using System.Drawing.Imaging;using System.Runtime.InteropServices;
public static class OfficeUI {
 private delegate bool Visitor(IntPtr h,IntPtr l);
 [DllImport("user32.dll")] private static extern bool EnumWindows(Visitor v,IntPtr l);
 [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] private static extern int GetWindowText(IntPtr h,StringBuilder b,int n);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h,int x,int y,int w,int z,bool repaint);
 [DllImport("user32.dll")] private static extern bool GetWindowRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,uint m,IntPtr w,IntPtr l);
 private struct RECT{public int left,top,right,bottom;}
 public static string Title(IntPtr h){var b=new StringBuilder(1024);GetWindowText(h,b,b.Capacity);return b.ToString();}
 public static IntPtr Find(int pid,string label){IntPtr found=IntPtr.Zero;EnumWindows((h,l)=>{uint p;GetWindowThreadProcessId(h,out p);if(p==(uint)pid&&IsWindowVisible(h)&&Title(h).IndexOf(label,StringComparison.OrdinalIgnoreCase)>=0){found=h;return false;}return true;},IntPtr.Zero);return found;}
 public static int[] Capture(IntPtr h,string path){RECT r;if(!GetWindowRect(h,out r))throw new Exception("No owned rectangle");using(var image=new Bitmap(r.right-r.left,r.bottom-r.top,PixelFormat.Format32bppArgb)){
  using(var g=Graphics.FromImage(image)){g.CopyFromScreen(r.left,r.top,0,0,image.Size);}image.Save(path,ImageFormat.Png);
  int orange=0,cyan=0;for(int y=0;y<image.Height;y+=2)for(int x=0;x<image.Width;x+=2){var p=image.GetPixel(x,y);if(p.R>210&&p.G>80&&p.G<160&&p.B<70)orange++;if(p.R<30&&p.G>150&&p.B>190)cyan++;}return new int[]{orange,cyan};}}
}
'@
function Wait-Host([string]$phase) {
 [IO.File]::WriteAllText("$r\PHASE.txt",$phase)
 $deadline=[DateTime]::UtcNow.AddSeconds(90)
 while(-not(Test-Path "$r\continue-$phase")) {
  if(Test-Path "$r\abort"){throw 'Host rejected captured UI evidence'}
  if([DateTime]::UtcNow-gt$deadline){throw "Host inspection deadline: $phase"}
  Start-Sleep -Milliseconds 200
 }
}
function Focus-Owned($window,$p) {
 $null=[OfficeUI]::SetForegroundWindow($window);Start-Sleep -Milliseconds 200
 [uint32]$foreground=0;$null=[OfficeUI]::GetWindowThreadProcessId([OfficeUI]::GetForegroundWindow(),[ref]$foreground)
 if($foreground-ne$p.Id){throw 'Owned Ottrin window does not have keyboard focus'}
}
function Wait-Colored($p,[string]$label,[string]$name) {
 $deadline=[DateTime]::UtcNow.AddSeconds(60);$window=[IntPtr]::Zero;$moved=$false;$pixels=@(0,0);$observed=@{}
 do {
  Start-Sleep -Milliseconds 300;$p.Refresh();if($p.HasExited){throw "Early Ottrin exit $($p.ExitCode)"}
  $conversions=@(Get-CimInstance Win32_Process -Filter "Name='office-render.exe'"|Where-Object{$_.ParentProcessId-eq$p.Id-and$_.CommandLine-and$_.CommandLine-notlike'*--version*'})
  foreach($child in $conversions){$observed[$child.ProcessId]=@{pid=$child.ProcessId;parent=$child.ParentProcessId;executable=$child.ExecutablePath;arguments=$child.CommandLine}}
  $window=[OfficeUI]::Find($p.Id,$label)
  if($window-ne[IntPtr]::Zero){
   if(-not$moved){$bounds=[Windows.Forms.Screen]::PrimaryScreen.Bounds;$null=[OfficeUI]::MoveWindow($window,20,20,[Math]::Min(1000,$bounds.Width-40),[Math]::Min(720,$bounds.Height-40),$true);Focus-Owned $window $p;$moved=$true}
   $pixels=[OfficeUI]::Capture($window,"$r\$name.png")
   if($pixels[0]-gt100-and$pixels[1]-gt100-and$observed.Count-gt0-and$conversions.Count-eq0){Start-Sleep -Seconds 2;$pixels=[OfficeUI]::Capture($window,"$r\$name.png");return @{window=$window;title=[OfficeUI]::Title($window);orange=$pixels[0];cyan=$pixels[1];conversions=@($observed.Values)}}
  }
 }while([DateTime]::UtcNow-lt$deadline)
 $report.wait_failure=@{conversions=@($observed.Values);orange=$pixels[0];cyan=$pixels[1]}
 throw "Source image halves not rendered: $name $pixels"
}
try {
 if($env:USERNAME-ne'campbell'){throw 'Wrong native actor'}
 $id=[Security.Principal.WindowsIdentity]::GetCurrent();if(([Security.Principal.WindowsPrincipal]::new($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Elevated GUI actor'}
 if([Diagnostics.Process]::GetCurrentProcess().SessionId-le0){throw 'Interactive desktop required'}
 if(@(Get-CimInstance Win32_Process|Where-Object{$_.Name-eq'ottrin-indexd.exe'}).Count){throw 'An existing daemon must not be displaced by private-profile QA'}
 $report.interactive=$true;$report.effective_administrator=$false;$report.filtered_administrator_membership=@($id.Groups|Where-Object{$_.Value-eq'S-1-5-32-544'}).Count-gt0
 $originalConfig="$env:APPDATA\ottrin\config.json";$before=(Get-FileHash $originalConfig).Hash
 $env:APPDATA="$r\profile\Roaming";$env:LOCALAPPDATA="$r\profile\Local";$env:TEMP="$r\temp";$env:TMP=$env:TEMP
 foreach($dir in @("$env:APPDATA\ottrin",$env:LOCALAPPDATA,$env:TEMP)){if(Test-Path $dir){throw 'Private profile already exists'};New-Item -ItemType Directory $dir -Force|Out-Null}
 $config=Get-Content $originalConfig -Raw|ConvertFrom-Json;$config.search.include_roots=@("$r\cases");$config.window_size=@(1000,720);$config.default_view_mode='Miller';$config|Add-Member -NotePropertyName installed_office_previews_enabled -NotePropertyValue $true -Force
 [IO.File]::WriteAllText("$env:APPDATA\ottrin\config.json",($config|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false))
 $env:OTTRIN_RENDERER='opengl';$env:OTTRIN_OFFICE_HELPER="$bin\ottrin-office-render.exe";$env:OTTRIN_PDFIUM_DIR="$bin\pdfium"
 $file="$r\cases\01-office.docx";$sourceHashes=@{};Get-ChildItem "$r\cases" -File|ForEach-Object{$sourceHashes[$_.Name]=(Get-FileHash $_.FullName).Hash}
 $p=Start-Process "$bin\ottrin.exe" -ArgumentList "--select `"$file`"" -PassThru -WorkingDirectory $bin -RedirectStandardOutput "$r\main.stdout" -RedirectStandardError "$r\main.stderr";$null=$p.Handle;$owned+=,$p
 $main=Wait-Colored $p 'Ottrin' 'main-rendered';$report.cases.main=@{title=$main.title;orange=$main.orange;cyan=$main.cyan;conversions=$main.conversions}
 Wait-Host 'main'
 $until=[DateTime]::UtcNow.AddSeconds(15)
 while(@(Get-CimInstance Win32_Process|Where-Object{$_.Name-eq'office-render.exe'}).Count){if([DateTime]::UtcNow-gt$until){throw 'Main conversion has not stopped'};Start-Sleep -Milliseconds 200}
 Rename-Item "$bin\ottrin-office-render.exe" 'ottrin-office-render.held'
 Focus-Owned $main.window $p;[Windows.Forms.SendKeys]::SendWait('{DOWN}');Start-Sleep -Seconds 5
 $null=[OfficeUI]::Capture($main.window,"$r\main-fallback.png")
 Wait-Host 'mainfallback'
 Rename-Item "$bin\ottrin-office-render.held" 'ottrin-office-render.exe'
 $report.cases.main_fallback=@{injection='Owned helper removed after successful main preview';review='Host must confirm worksheet and fallback notice before continuing'}
 $null=[OfficeUI]::PostMessage($main.window,0x10,[IntPtr]::Zero,[IntPtr]::Zero);if(-not$p.WaitForExit(15000)-or$p.ExitCode-ne0){throw 'Main window did not close cleanly'}
 $p=Start-Process "$bin\ottrin.exe" -ArgumentList "preview `"$file`"" -PassThru -WorkingDirectory $bin -RedirectStandardOutput "$r\inspector.stdout" -RedirectStandardError "$r\inspector.stderr";$null=$p.Handle;$owned+=,$p
 $inspector=Wait-Colored $p '01-office.docx' 'inspector-rendered';$report.cases.inspector=@{title=$inspector.title;orange=$inspector.orange;cyan=$inspector.cyan;conversions=$inspector.conversions}
 Wait-Host 'inspector'
 $until=[DateTime]::UtcNow.AddSeconds(15)
 while(@(Get-CimInstance Win32_Process|Where-Object{$_.Name-eq'office-render.exe'}).Count){if([DateTime]::UtcNow-gt$until){throw 'Office conversion has not stopped'};Start-Sleep -Milliseconds 200}
 Rename-Item "$bin\ottrin-office-render.exe" 'ottrin-office-render.held'
 Focus-Owned $inspector.window $p;[Windows.Forms.SendKeys]::SendWait('{RIGHT}')
 $until=[DateTime]::UtcNow.AddSeconds(15)
 do {Start-Sleep -Milliseconds 200;$title=[OfficeUI]::Title($inspector.window);if($title-like'*02-office.xlsx*'){break}}while([DateTime]::UtcNow-lt$until)
 if($title-notlike'*02-office.xlsx*'){throw "Inspector did not navigate to fallback worksheet: $title"}
 Start-Sleep -Seconds 5;$null=[OfficeUI]::Capture($inspector.window,"$r\inspector-fallback.png");$report.cases.fallback=@{title=$title;injection='Owned helper removed after successful provider detection/conversion'}
 Wait-Host 'fallback'
 $null=[OfficeUI]::PostMessage($inspector.window,0x10,[IntPtr]::Zero,[IntPtr]::Zero);if(-not$p.WaitForExit(15000)-or$p.ExitCode-ne0){throw 'Inspector did not close cleanly'}
 foreach($name in $sourceHashes.Keys){if((Get-FileHash "$r\cases\$name").Hash-ne$sourceHashes[$name]){throw 'Source bytes changed'}}
 if((Get-FileHash $originalConfig).Hash-ne$before){throw 'Original configuration changed'}
 $report.temporary_entries=@(Get-ChildItem "$r\temp" -Force).Count;if($report.temporary_entries){throw 'Preview conversion left private temporary files'}
 $report.source_sha256=$sourceHashes;$report.original_profile_sha256=$before;$report.status='pass'
}catch{$report.status='fail';$report.error=$_.Exception.ToString();$report.stack=$_.ScriptStackTrace}
finally {
 foreach($p in $owned){if(-not$p.HasExited){& C:\Windows\System32\taskkill.exe /PID $p.Id /T /F|Out-Null;$null=$p.WaitForExit(10000)}}
 if(Test-Path "$bin\ottrin-office-render.held"){Rename-Item "$bin\ottrin-office-render.held" 'ottrin-office-render.exe'}
 try {
  $daemon=@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-eq"$bin\ottrin-indexd.exe"})
  if($daemon.Count){$stop=Start-Process "$bin\ottrin-indexd.exe" -ArgumentList '--stop' -PassThru -WindowStyle Hidden;$null=$stop.Handle;if(-not$stop.WaitForExit(15000)-or$stop.ExitCode-ne0){throw 'Owned daemon did not stop cleanly'}}
  $until=[DateTime]::UtcNow.AddSeconds(10)
  do{$live=@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like"$r\*"});if(-not$live.Count){break};Start-Sleep -Milliseconds 200}while([DateTime]::UtcNow-lt$until)
  if($live.Count){throw 'Owned process remains'}
  $report.owned_processes=0
  foreach($dir in @("$r\profile","$r\temp")){if(Test-Path $dir){Remove-Item -LiteralPath $dir -Recurse -Force}}
  $report.private_profile_removed=(-not(Test-Path "$r\profile"));$report.private_temp_removed=(-not(Test-Path "$r\temp"))
 }catch{$report.status='fail';$report.cleanup_error=$_.Exception.ToString()}
 $report|ConvertTo-Json -Depth 8|Set-Content "$r\RESULTS.json"
}
if($report.status-ne'pass'){exit 1}
