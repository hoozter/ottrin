$ErrorActionPreference='Stop'
$root='C:\OttrinJobFixtureQA';$phase=(Get-Content "$root\phase" -Raw).Trim()
Start-Transcript -Path "$root\results\$phase.transcript"|Out-Null
$identity=[Security.Principal.WindowsIdentity]::GetCurrent();$owner=Get-Content "$root\INTERACTIVE_OWNER.json" -Raw|ConvertFrom-Json
if($identity.User.Value-ne$owner.sid-or([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Expected ordinary interactive owner'}
$homeRoot=Join-Path $root "data\$phase";New-Item -ItemType Directory $homeRoot|Out-Null
foreach($pair in @(@('TEMP','t'),@('TMP','t'),@('APPDATA','a'),@('LOCALAPPDATA','l'),@('USERPROFILE','h'),@('HOME','h'))){$path=Join-Path $homeRoot $pair[1];New-Item -ItemType Directory $path -Force|Out-Null;[Environment]::SetEnvironmentVariable($pair[0],$path,'Process')}
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
Add-Type -ReferencedAssemblies System.Windows.Forms,System.Drawing -TypeDefinition @'
using System;using System.Text;using System.Runtime.InteropServices;using System.Windows.Forms;using System.Drawing;using System.Drawing.Imaging;
public static class PickerNative {
 public delegate bool EnumProc(IntPtr hwnd,IntPtr parameter);
 [StructLayout(LayoutKind.Sequential)]public struct Rect {public int left,top,right,bottom;}
 [StructLayout(LayoutKind.Sequential)]public struct Point {public int x,y;}
 [DllImport("user32.dll")]static extern bool EnumWindows(EnumProc callback,IntPtr param);
 [DllImport("user32.dll")]static extern uint GetWindowThreadProcessId(IntPtr hwnd,out uint pid);
 [DllImport("user32.dll")]static extern bool IsWindowVisible(IntPtr hwnd);
 [DllImport("user32.dll")]public static extern bool IsWindowEnabled(IntPtr hwnd);
 [DllImport("user32.dll")]public static extern IntPtr GetWindow(IntPtr hwnd,uint command);
 [DllImport("user32.dll")]static extern bool GetClientRect(IntPtr hwnd,out Rect rect);
 [DllImport("user32.dll")]static extern bool ClientToScreen(IntPtr hwnd,ref Point point);
 [DllImport("user32.dll")]static extern bool GetWindowRect(IntPtr hwnd,out Rect rect);
 [DllImport("user32.dll")]public static extern bool SetForegroundWindow(IntPtr hwnd);
 [DllImport("user32.dll")]static extern void mouse_event(uint flags,uint x,uint y,uint data,UIntPtr extra);
 [DllImport("user32.dll")]static extern uint GetDpiForWindow(IntPtr hwnd);
 [DllImport("user32.dll")]static extern bool SetProcessDPIAware();
 public static void Init(){SetProcessDPIAware();}
 public static IntPtr Window(int wanted){IntPtr found=IntPtr.Zero;EnumWindows(delegate(IntPtr h,IntPtr p){uint id;GetWindowThreadProcessId(h,out id);if(id==wanted&&IsWindowVisible(h)){found=h;return false;}return true;},IntPtr.Zero);return found;}
 public static void Click(IntPtr h,double x,double y){SetForegroundWindow(h);Rect r;GetClientRect(h,out r);double scale=GetDpiForWindow(h)/96.0;var p=new Point();p.x=(int)(x<0?r.right+x*scale:x*scale);p.y=(int)(y<0?r.bottom+y*scale:y*scale);ClientToScreen(h,ref p);Cursor.Position=new System.Drawing.Point(p.x,p.y);mouse_event(2,0,0,0,UIntPtr.Zero);mouse_event(4,0,0,0,UIntPtr.Zero);}
 public static void Shot(IntPtr h,string path){Rect r;GetWindowRect(h,out r);using(var b=new Bitmap(r.right-r.left,r.bottom-r.top))using(var g=Graphics.FromImage(b)){g.CopyFromScreen(r.left,r.top,0,0,b.Size);b.Save(path,ImageFormat.Png);}}
}
'@
[PickerNative]::Init()
function Native([string]$value){return @{encoding='windows-utf16-le';units_hex=(-join($value.ToCharArray()|ForEach-Object {'{0:x4}' -f [int]$_}))}}
function Pump {[Windows.Forms.Application]::DoEvents();Start-Sleep -Milliseconds 30}
$actors=@();$forms=@();$report=@{scope='Actual ordinary-token cooperating caller, source picker, native owner, Open/Save, EOF and owner loss';cases=@()}
$corpus=Join-Path $homeRoot 'corpus';New-Item -ItemType Directory $corpus|Out-Null;$source=Join-Path $corpus 'caller-file.txt';[IO.File]::WriteAllText($source,'preserve native caller source');$hash=(Get-FileHash $source).Hash
$indexBefore=@(Get-CimInstance Win32_Process|Where-Object {$_.Name-ieq'ottrin-indexd.exe'}|ForEach-Object ProcessId|Sort-Object)
try {
 foreach($case in @('open','save','disconnect','owner-loss')) {
  $form=New-Object Windows.Forms.Form;$forms+=,$form;$form.Text='Owned Ottrin caller '+$case;$form.Size=New-Object Drawing.Size(500,300);$form.Show();[Windows.Forms.Application]::DoEvents();$parent=$form.Handle
  $request=@{mode=$(if($case-eq'save'){'save'}else{'open'});title='Native caller '+$case;parent_window=('win32:{0:x}' -f $parent.ToInt64());initial_directory=(Native $corpus);filename=(Native $(if($case-eq'save'){'caller-save.txt'}else{''}))}
  $info=New-Object Diagnostics.ProcessStartInfo;$info.FileName="$root\fixture-launch.exe";$info.Arguments='picker --cancel-on-disconnect --renderer opengl';$info.UseShellExecute=$false;$info.CreateNoWindow=$true;$info.RedirectStandardInput=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true;$info.WorkingDirectory=$homeRoot
  $process=New-Object Diagnostics.Process;$actors+=,$process;$process.StartInfo=$info;if(-not$process.Start()){throw 'Picker did not start'};$stdout=$process.StandardOutput.ReadToEndAsync();$stderr=$process.StandardError.ReadToEndAsync();$process.StandardInput.AutoFlush=$true;$process.StandardInput.WriteLine(($request|ConvertTo-Json -Depth 8 -Compress))
  $deadline=[DateTime]::UtcNow.AddSeconds(30);$child=[IntPtr]::Zero
  while($child-eq[IntPtr]::Zero-and[DateTime]::UtcNow-lt$deadline){if($process.HasExited){throw ('Picker exited early: '+$stderr.Result)};Pump;$child=[PickerNative]::Window($process.Id)}
  if($child-eq[IntPtr]::Zero){throw 'Picker window not visible'}
  $deadline=[DateTime]::UtcNow.AddSeconds(5);while([PickerNative]::GetWindow($child,4)-ne$parent-and[DateTime]::UtcNow-lt$deadline){Pump}
  if([PickerNative]::GetWindow($child,4)-ne$parent){throw 'Native owner mismatch'};if(-not[PickerNative]::IsWindowEnabled($parent)){throw 'Picker changed caller enabled state'}
  for($i=0;$i-lt12;$i++){Pump};[PickerNative]::Shot($child,"$root\results\$phase-$case.png")
  switch($case){
   'open' {[PickerNative]::Click($child,300,154);for($i=0;$i-lt5;$i++){Pump};[PickerNative]::Click($child,-37,-33)}
   'save' {[PickerNative]::Click($child,-37,-33)}
   'disconnect' {$process.StandardInput.Close()}
   'owner-loss' {$form.Close();$form.Dispose()}
  }
  $deadline=[DateTime]::UtcNow.AddSeconds(15);while(-not$process.HasExited-and[DateTime]::UtcNow-lt$deadline){Pump};if(-not$process.HasExited){throw ('Picker remained open after '+$case)};$process.WaitForExit()
  $raw=$stdout.Result;$raw|Set-Content "$root\results\$phase-$case.stdout";$stderr.Result|Set-Content "$root\results\$phase-$case.stderr";if($process.ExitCode-ne0){throw ('Picker exit '+$process.ExitCode+': '+$stderr.Result)}
  $response=$raw|ConvertFrom-Json;$accepted=$case-in@('open','save');if($response.accepted-ne$accepted){throw ('Wrong response: '+$raw)}
  if($accepted){$expected=if($case-eq'open'){$source}else{Join-Path $corpus 'caller-save.txt'};if($response.paths.Count-ne1-or($response.paths[0].units_hex-ne(Native $expected).units_hex-and$response.paths[0].units_hex-ne(Native ('\\?\'+$expected)).units_hex)){throw ('Incorrect native target: '+$raw)};if($case-eq'save'){if(Test-Path $expected){throw 'Picker created saved file'};[IO.File]::WriteAllText($expected,'saved by cooperating caller')}}
  $report.cases+=@{case=$case;exit=$process.ExitCode;accepted=$response.accepted;nativeOwnerVerified=$true;callerEnabledStatePreserved=$true;response=$response};if(-not$form.IsDisposed){$form.Close();$form.Dispose()}
 }
 if((Get-FileHash $source).Hash-ne$hash){throw 'Source modified'};if(Test-Path (Join-Path $env:APPDATA 'ottrin\config.json')){throw 'Main config unexpectedly written'}
 $indexAfter=@(Get-CimInstance Win32_Process|Where-Object {$_.Name-ieq'ottrin-indexd.exe'}|ForEach-Object ProcessId|Sort-Object);if(($indexBefore-join',')-ne($indexAfter-join',')){throw 'Picker changed index process state'}
 $report.status='pass';$report.originalPreserved=$true;$report.mainConfigUnchanged=$true;$report.indexStateUnchanged=$true;$report|ConvertTo-Json -Depth 15|Set-Content "$root\results\$phase.json"
} catch { ($_|Out-String)+$_.ScriptStackTrace|Set-Content "$root\results\$phase-error.txt";throw } finally {
 foreach($process in $actors){if(-not$process.HasExited){& C:\Windows\System32\taskkill.exe /PID $process.Id /T /F|Out-Null;$null=$process.WaitForExit(10000)};$process.Dispose()}
 foreach($form in $forms){if(-not$form.IsDisposed){$form.Close();$form.Dispose()}}
 Remove-Item -LiteralPath $homeRoot -Recurse -Force
}
