param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExpectedSid)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$out=Join-Path $Root 'results';$report=@{status='running';scope='Exact installed native pointer resize at visible rounded arcs and exclusive titlebar ownership;0/24radius,Native/OpenGL; source/preferences preservation';cases=@()};$final='failed native pointer-resize audit';$owned=[Collections.Generic.List[Diagnostics.Process]]::new();$install=$null
function Require($value,$message){if(-not$value){throw $message}}
function Save{$p=Join-Path $out 'RESULT.json';$tmp=$p+'.next';[IO.File]::WriteAllText($tmp,($report|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false));if(Test-Path $p){[IO.File]::Replace($tmp,$p,[NullString]::Value)}else{[IO.File]::Move($tmp,$p)}}
function Phase($value){$report.phase=$value;Save}
function Run($exe,$arguments,$name){
 $s=[Diagnostics.ProcessStartInfo]::new($exe,$arguments);$s.UseShellExecute=$false;$s.CreateNoWindow=$true;$s.RedirectStandardOutput=$true;$s.RedirectStandardError=$true;$s.StandardOutputEncoding=[Text.UTF8Encoding]::new($false);$s.StandardErrorEncoding=[Text.UTF8Encoding]::new($false);$clock=[Diagnostics.Stopwatch]::StartNew();$p=[Diagnostics.Process]::Start($s)
 try{
  $stdout=$p.StandardOutput.ReadToEndAsync();$stderr=$p.StandardError.ReadToEndAsync()
  if(-not$p.WaitForExit(180000)){
   $snapshot=@(Get-CimInstance Win32_Process|Where-Object {$_.ProcessId-eq$p.Id-or$_.ParentProcessId-eq$p.Id-or($install-and$_.ExecutablePath-and$_.ExecutablePath.StartsWith($install,[StringComparison]::OrdinalIgnoreCase))}|Select-Object Name,ProcessId,ParentProcessId,ExecutablePath,SessionId)
   [IO.File]::WriteAllText((Join-Path $out ($name+'-timeout.json')),(@{seconds=$clock.Elapsed.TotalSeconds;root_pid=$p.Id;processes=$snapshot}|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false));throw ('Owned process timeout '+$name)
  }
  Require ($stdout.Wait(5000)-and$stderr.Wait(5000)) 'Process streams not complete';$v=@{exit=$p.ExitCode;stdout=$stdout.Result;stderr=$stderr.Result;seconds=$clock.Elapsed.TotalSeconds};[IO.File]::WriteAllText((Join-Path $out ($name+'-process.json')),($v|ConvertTo-Json),[Text.UTF8Encoding]::new($false));Require ($v.exit-eq0) ('Native child failed '+$name);return $v
 }finally{if(-not$p.HasExited){& "$env:SystemRoot\System32\taskkill.exe" /PID $p.Id /T /F|Out-Null;$null=$p.WaitForExit(10000)};$p.Dispose()}
}
& ([ScriptBlock]::Create([IO.File]::ReadAllText((Join-Path $Root 'native-controls.ps1'))));[PickerNative]::Init()
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
function Pump{[Windows.Forms.Application]::DoEvents();Start-Sleep -Milliseconds 50}
function Tree($w){return [Windows.Automation.AutomationElement]::FromHandle($w)}
function Nodes($w,$name){$c=[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,$name);return @((Tree $w).FindAll([Windows.Automation.TreeScope]::Descendants,$c)|Where-Object {-not$_.Current.IsOffscreen})}
function Node($w,$name,$kind){$deadline=[DateTime]::UtcNow.AddSeconds(20);do{$n=@(Nodes $w $name|Where-Object {$_.Current.ControlType-eq$kind});if($n.Count-eq1){return $n[0]};Pump}while([DateTime]::UtcNow-lt$deadline);[PickerNative]::Shot($w,(Join-Path $out 'missing-control.png'));throw ('Missing unique '+$name+' count='+$n.Count)}
function Invoke($w,$name){$n=Node $w $name ([Windows.Automation.ControlType]::Button);$n.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke();Pump}
function Key($w,$key){[PickerNative]::Focus($w);[Windows.Forms.SendKeys]::SendWait($key);Start-Sleep -Milliseconds 300;Pump}
try{
 Require ([Security.Principal.WindowsIdentity]::GetCurrent().User.Value-eq$ExpectedSid) 'Wrong owned account';Require (-not([Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Ordinary token required'
 $identity=Get-Content (Join-Path $Root 'application-input.json') -Raw|ConvertFrom-Json;foreach($file in $identity.files){Require ((Get-FileHash (Join-Path $Root $file.name)).Hash.ToLowerInvariant()-eq$file.sha256) 'Native actor input mismatch'};$report.package=$identity
 $manifest=Get-Content (Join-Path $Root 'payload.json') -Raw|ConvertFrom-Json;$install=Join-Path $env:LOCALAPPDATA 'Programs\Ottrin';$app=Join-Path $install 'bin\ottrin.exe';Require (-not(Test-Path $install)) 'Existing installation retained'
 Phase 'install exact staged NSIS';$null=Run (Join-Path $Root 'current-setup.exe') '/S' 'install'
 foreach($file in $manifest.PSObject.Properties){Require ((Get-FileHash (Join-Path $install $file.Name)).Hash.ToLowerInvariant()-eq$file.Value) ('Payload mismatch '+$file.Name)}
 $null=Run $app 'index service-remove' 'index-remove';Require (@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue).Count-eq0) 'Owned index process remains'
 $corpus=Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'OttrinFields';Require (-not(Test-Path $corpus)) 'Existing corpus retained';$null=[IO.Directory]::CreateDirectory($corpus);$source=Join-Path $corpus 'preserve.txt';[IO.File]::WriteAllText($source,'owned original',[Text.UTF8Encoding]::new($false));$sourceHash=(Get-FileHash $source).Hash
 $config=Get-Content (Join-Path $Root 'config-template.json') -Raw|ConvertFrom-Json;$config.bookmarks=@();$config.default_view_mode='List';$config.graphics_renderer='Native';$config.search.include_roots=@($corpus);$config.search.content_indexing_enabled=$false;$pref=Join-Path $env:APPDATA 'ottrin\config.json';Require (-not(Test-Path $pref)) 'Existing config retained';$null=[IO.Directory]::CreateDirectory((Split-Path $pref))



 Add-Type -ReferencedAssemblies System.Windows.Forms,System.Drawing @'
using System;using System.Drawing;using System.Runtime.InteropServices;using System.Threading;
public static class ResizeGesture {
[StructLayout(LayoutKind.Sequential)]struct Rect {public int left,top,right,bottom;}
[DllImport("user32.dll")]static extern bool GetWindowRect(IntPtr w,out Rect r);
[DllImport("user32.dll")]static extern bool SetWindowPos(IntPtr h,IntPtr after,int x,int y,int width,int height,uint flags);
[DllImport("user32.dll")]static extern void mouse_event(uint flags,uint x,uint y,uint data,UIntPtr extra);
public static int[] Bounds(IntPtr w){Rect r;if(!GetWindowRect(w,out r))throw new Exception("Owned bounds unavailable");return new int[]{r.left,r.top,r.right-r.left,r.bottom-r.top};}
public static void Size(IntPtr w,int width,int height){if(!SetWindowPos(w,IntPtr.Zero,0,0,width,height,0x16))throw new Exception("Cannot size owned window");Thread.Sleep(1000);}
public static int[] Drag(IntPtr w,int x,int y,int dx,int dy){var b=Bounds(w);var point=new Point(b[0]+x,b[1]+y);System.Windows.Forms.Cursor.Position=point;Thread.Sleep(1000);mouse_event(2,0,0,0,UIntPtr.Zero);try{Thread.Sleep(1000);System.Windows.Forms.Cursor.Position=new Point(point.X+dx,point.Y+dy);Thread.Sleep(1000);}finally{mouse_event(4,0,0,0,UIntPtr.Zero);}Thread.Sleep(1000);return Bounds(w);}
}
'@
 function Await($pidValue,$title){$deadline=[DateTime]::UtcNow.AddSeconds(25);do{$v=[PickerNative]::Window($pidValue,$title);if($v-ne[IntPtr]::Zero){return $v};Pump}while([DateTime]::UtcNow-lt$deadline);throw ('Missing owned window '+$title)}
 function Gesture($w,$name,$radius,$corner,$dx,$dy){[PickerNative]::Place($w);[PickerNative]::Focus($w);$before=[ResizeGesture]::Bounds($w);$x=if($corner-eq'SE'){$before[2]-[Math]::Max($radius,4)}else{[Math]::Max($radius,4)};$y=if($corner-eq'SE'){$before[3]-4}else{4};$after=[ResizeGesture]::Drag($w,$x,$y,$dx,$dy);$capture=($renderer+'-'+$name+'-'+$radius+'.png');[PickerNative]::Shot($w,(Join-Path $out $capture));$report.cases+=@{renderer=$renderer;radius=$radius;case=$name;corner=$corner;pointer_start=@($x,$y);pointer_delta=@($dx,$dy);before=$before;after=$after;capture=$capture};Save
  if($corner-eq'SE'){Require ([Math]::Abs($after[2]-$before[2]-$dx)-le2-and[Math]::Abs($after[3]-$before[3]-$dy)-le2) 'Visible arc did not resize both axes';Require ([Math]::Abs($after[0]-$before[0])-le2-and[Math]::Abs($after[1]-$before[1])-le2) 'Resize moved window origin'}else{Require ([Math]::Abs($after[0]-$before[0]-$dx)-le2-and[Math]::Abs($after[1]-$before[1]-$dy)-le2) 'Northwest resize origin incorrect';Require ([Math]::Abs($after[2]-$before[2]+$dx)-le2-and[Math]::Abs($after[3]-$before[3]+$dy)-le2) 'Titlebar drag stole resize: dimensions did not shrink on both axes'}
 }
 foreach($renderer in @('Native','OpenGL')){foreach($radius in @(0,24)){
  Phase ($renderer+' pointer radius '+$radius);$config.theme='Dark';$config.graphics_renderer=if($renderer-eq'Native'){'Native'}else{'OpenGlCompatibility'};$config.active_custom_theme='Radius audit';$config.custom_themes=@(@{name='Radius audit';base_preset='Ottrin';dark=@{app_radius=$radius};light=@{app_radius=$radius}});$rendererArgs=if($renderer-eq'OpenGL'){' --renderer opengl'}else{''};[IO.File]::WriteAllText($pref,($config|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false))
  $s=[Diagnostics.ProcessStartInfo]::new($app,('"'+$corpus+'"'+$rendererArgs));$s.UseShellExecute=$false;$p=[Diagnostics.Process]::Start($s);$owned.Add($p);$w=Await $p.Id 'Ottrin';$null=Node $w 'Menu' ([Windows.Automation.ControlType]::Button);[ResizeGesture]::Size($w,1000,650);Gesture $w 'main-visible-arc-SE' $radius 'SE' 40 30
  Invoke $w 'Menu';Invoke $w ('Settings'+[char]0x2026);$settings=Await $p.Id 'Ottrin Settings';$null=Node $settings 'General' ([Windows.Automation.ControlType]::RadioButton);Gesture $settings 'settings-titlebar-NW' $radius 'NW' 20 20;Invoke $settings 'Close window';Start-Sleep -Milliseconds 500
  Invoke $w 'Close window';Require ($p.WaitForExit(15000)-and$p.ExitCode-eq0) 'Native main close failed';Require ((Get-FileHash $source).Hash-eq$sourceHash) 'Source changed';Save
 }}
 $saved=(Get-FileHash $pref).Hash;Phase 'supported uninstall';$uninstaller=Join-Path $out 'owned-uninstall.exe';Copy-Item (Join-Path $install 'Uninstall.exe') $uninstaller;$null=Run $uninstaller ('/S _?='+$install) 'uninstall';Remove-Item $uninstaller;Require (-not(Test-Path $install)) 'Installation remains';Require ((Get-FileHash $pref).Hash-eq$saved) 'Uninstall changed preferences';Require ((Get-FileHash $source).Hash-eq$sourceHash) 'Source changed';$report.preferences_preserved=$true;$report.source_preserved=$true;$final='pass native installed pointer resize andexclusive titlebarownership; originalsrequire review'
}catch{$report.error=$_.Exception.Message;$report.stack=$_.ScriptStackTrace}
finally{
 if($picker){try{if($picker.HasExited-and$stdout-and$stderr){$null=$stdout.Wait(5000);$null=$stderr.Wait(5000);$report.picker_child=@{exit=$picker.ExitCode;stdout=$stdout.Result;stderr=$stderr.Result}}}catch{$report.picker_diagnostic_error=$_.Exception.Message}}
 foreach($p in $owned){if(-not$p.HasExited){& "$env:SystemRoot\System32\taskkill.exe" /PID $p.Id /T /F|Out-Null;$null=$p.WaitForExit(10000)}}
 if($install-and(Test-Path $app)-and$manifest-and(Get-FileHash $app).Hash.ToLowerInvariant()-eq$manifest.'bin/ottrin.exe'){try{$report.cleanup_index_remove=Run $app 'index service-remove' 'cleanup-index-remove'}catch{$report.cleanup_index_error=$_.Exception.Message;$final='failed owned index teardown'}}
 if($install){$left=@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath-and$_.ExecutablePath.StartsWith($install,[StringComparison]::OrdinalIgnoreCase)});$report.remaining_owned_installed_processes=$left.Count;if($left.Count){$final='failed owned process teardown'}}
 $report.status=$final;Save
}
if(-not$final.StartsWith('pass')){exit 1}
