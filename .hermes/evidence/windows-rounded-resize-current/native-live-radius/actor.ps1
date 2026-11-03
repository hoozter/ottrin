param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExpectedSid)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$out=Join-Path $Root 'results';$report=@{status='running';scope='Exact installed native live editor radius and Cancel propagation;0/24radius,Native/OpenGL; source/preferences preservation';cases=@()};$final='failed native pointer-resize audit';$owned=[Collections.Generic.List[Diagnostics.Process]]::new();$install=$null
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



 Add-Type @'
using System;using System.Runtime.InteropServices;
public static class RadiusProbe {
[StructLayout(LayoutKind.Sequential)]struct Rect {public int left,top,right,bottom;}
[DllImport("gdi32.dll")]static extern IntPtr CreateRectRgn(int a,int b,int c,int d);
[DllImport("gdi32.dll")]static extern IntPtr CreateRoundRectRgn(int a,int b,int c,int d,int e,int f);
[DllImport("gdi32.dll")]static extern bool EqualRgn(IntPtr a,IntPtr b);
[DllImport("user32.dll")]static extern int GetWindowRgn(IntPtr w,IntPtr r);
[DllImport("user32.dll")]static extern bool GetWindowRect(IntPtr w,out Rect r);
[DllImport("gdi32.dll")]static extern bool PtInRegion(IntPtr r,int x,int y);
[DllImport("gdi32.dll")]static extern bool DeleteObject(IntPtr r);
[DllImport("user32.dll")]static extern bool SetWindowPos(IntPtr h,IntPtr after,int x,int y,int width,int height,uint flags);
[DllImport("user32.dll")]public static extern bool IsZoomed(IntPtr h);
[DllImport("user32.dll")]public static extern uint GetGuiResources(IntPtr h,uint flags);
public static void Resize(IntPtr h,int width,int height){if(!SetWindowPos(h,IntPtr.Zero,0,0,width,height,0x16))throw new Exception("Owned window resize failed");}
public static bool Matches(IntPtr w,int radius){Rect b;if(!GetWindowRect(w,out b))throw new Exception("No owned window bounds");int width=b.right-b.left,height=b.bottom-b.top;var expected=radius==0?CreateRectRgn(0,0,width,height):CreateRoundRectRgn(0,0,width+1,height+1,radius*2,radius*2);var actual=CreateRectRgn(0,0,0,0);try{return GetWindowRgn(w,actual)!=0&&EqualRgn(expected,actual);}finally{DeleteObject(expected);DeleteObject(actual);}}
public static string Region(IntPtr w){var r=CreateRectRgn(0,0,0,0);if(r==IntPtr.Zero)throw new Exception("Query region allocation failed");try{Rect bounds;if(!GetWindowRect(w,out bounds))throw new Exception("No owned window bounds");int kind=GetWindowRgn(w,r);int width=bounds.right-bounds.left,height=bounds.bottom-bounds.top;return "kind="+kind+" topLeft="+PtInRegion(r,1,1)+" topRight="+PtInRegion(r,width-2,1)+" bottomLeft="+PtInRegion(r,1,height-2)+" bottomRight="+PtInRegion(r,width-2,height-2)+" center="+PtInRegion(r,width/2,height/2)+" dimensions="+width+"x"+height;}finally{DeleteObject(r);}}
}
'@
 function Await($pidValue,$title){$deadline=[DateTime]::UtcNow.AddSeconds(25);do{$v=[PickerNative]::Window($pidValue,$title);if($v-ne[IntPtr]::Zero){return $v};Pump}while([DateTime]::UtcNow-lt$deadline);throw ('Missing owned window '+$title)}

 function CaptureLive($handle,$label,$radius){[PickerNative]::Focus($handle);Start-Sleep -Milliseconds 500;$matches=[RadiusProbe]::Matches($handle,$radius);$capture=$renderer+'-'+$label+'-'+$radius+'.png';[PickerNative]::Shot($handle,(Join-Path $out $capture));$report.cases+=@{renderer=$renderer;case=$label;radius=$radius;native_expected_region_match=$matches;native_region=[RadiusProbe]::Region($handle);capture=$capture};Save;Require $matches ('Live radius mismatch '+$label+' expected '+$radius)}
 function SetCorners($editor,$value){
  $label=Node $editor 'Window corners' ([Windows.Automation.ControlType]::Text);$bounds=$label.Current.BoundingRectangle
  $all=@((Tree $editor).FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)|Where-Object {-not$_.Current.IsOffscreen})
  $numeric=@($all|Where-Object {$b=$_.Current.BoundingRectangle;($_.Current.ControlType-eq[Windows.Automation.ControlType]::Spinner-or$_.Current.ControlType-eq[Windows.Automation.ControlType]::Edit)-and$b.Left-ge$bounds.Right-and[Math]::Abs(($b.Top+$b.Height/2)-($bounds.Top+$bounds.Height/2))-lt16})
  if($numeric.Count-ne1){$all|ForEach-Object {@{name=$_.Current.Name;type=$_.Current.ControlType.ProgrammaticName;bounds=$_.Current.BoundingRectangle.ToString()}}|ConvertTo-Json -Depth 5|Set-Content (Join-Path $out 'numeric-tree.json') -Encoding UTF8;throw ('Window corners numeric control count='+$numeric.Count)}
  $numeric[0].SetFocus();[Windows.Forms.SendKeys]::SendWait('^a');[Windows.Forms.SendKeys]::SendWait([string]$value);[Windows.Forms.SendKeys]::SendWait('{ENTER}');Start-Sleep -Milliseconds 700;Pump
 }
 foreach($renderer in @('Native','OpenGL')){
  Phase ($renderer+' live editor radius');$config.theme='Dark';$config.graphics_renderer=if($renderer-eq'Native'){'Native'}else{'OpenGlCompatibility'};$config.active_custom_theme='Radius audit';$config.custom_themes=@(@{name='Radius audit';base_preset='Ottrin';dark=@{app_radius=0};light=@{app_radius=0}});$rendererArgs=if($renderer-eq'OpenGL'){' --renderer opengl'}else{''};[IO.File]::WriteAllText($pref,($config|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false))
  $s=[Diagnostics.ProcessStartInfo]::new($app,('"'+$corpus+'"'+$rendererArgs));$s.UseShellExecute=$false;$p=[Diagnostics.Process]::Start($s);$owned.Add($p);$w=Await $p.Id 'Ottrin';$null=Node $w 'Menu' ([Windows.Automation.ControlType]::Button)
  Invoke $w 'Menu';Invoke $w ('Settings'+[char]0x2026);$settings=Await $p.Id 'Ottrin Settings'
  (Node $settings 'Appearance' ([Windows.Automation.ControlType]::RadioButton)).GetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern).Select();Pump
  $buttons=@();for($i=0;$i-lt12;$i++){$buttons=@(Nodes $settings 'Edit Radius audit'|Where-Object {$_.Current.ControlType-eq[Windows.Automation.ControlType]::Button});if($buttons.Count-eq1){break};[PickerNative]::Wheel($settings,-480);Pump};Require ($buttons.Count-eq1) 'Saved theme edit not visible';$buttons[0].GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke();$editor=Await $p.Id 'Ottrin Theme Editor';[RadiusProbe]::Resize($editor,1180,760);Pump
  foreach($radius in @(24,0)){SetCorners $editor $radius;CaptureLive $editor 'editor-live' $radius;CaptureLive $settings 'settings-live' $radius;CaptureLive $w 'main-live' $radius}
  SetCorners $editor 24;CaptureLive $editor 'editor-before-cancel' 24;Invoke $editor 'Cancel';Start-Sleep -Milliseconds 700;CaptureLive $settings 'settings-after-cancel' 0;CaptureLive $w 'main-after-cancel' 0
  Invoke $settings 'Close window';Invoke $w 'Close window';Require ($p.WaitForExit(15000)-and$p.ExitCode-eq0) 'Normal close failed';$savedConfig=Get-Content $pref -Raw|ConvertFrom-Json;Require ($savedConfig.custom_themes[0].dark.app_radius-eq0) 'Cancel persisted unsaved radius';Require ((Get-FileHash $source).Hash-eq$sourceHash) 'Source changed';Save
 }
 $saved=(Get-FileHash $pref).Hash;Phase 'supported uninstall';$uninstaller=Join-Path $out 'owned-uninstall.exe';Copy-Item (Join-Path $install 'Uninstall.exe') $uninstaller;$null=Run $uninstaller ('/S _?='+$install) 'uninstall';Remove-Item $uninstaller;Require (-not(Test-Path $install)) 'Installation remains';Require ((Get-FileHash $pref).Hash-eq$saved) 'Uninstall changed preferences';Require ((Get-FileHash $source).Hash-eq$sourceHash) 'Source changed';$report.preferences_preserved=$true;$report.source_preserved=$true;$final='pass native live radius propagation and Cancel; originalsrequire review'
}catch{$report.error=$_.Exception.Message;$report.stack=$_.ScriptStackTrace}
finally{
 if($picker){try{if($picker.HasExited-and$stdout-and$stderr){$null=$stdout.Wait(5000);$null=$stderr.Wait(5000);$report.picker_child=@{exit=$picker.ExitCode;stdout=$stdout.Result;stderr=$stderr.Result}}}catch{$report.picker_diagnostic_error=$_.Exception.Message}}
 foreach($p in $owned){if(-not$p.HasExited){& "$env:SystemRoot\System32\taskkill.exe" /PID $p.Id /T /F|Out-Null;$null=$p.WaitForExit(10000)}}
 if($install-and(Test-Path $app)-and$manifest-and(Get-FileHash $app).Hash.ToLowerInvariant()-eq$manifest.'bin/ottrin.exe'){try{$report.cleanup_index_remove=Run $app 'index service-remove' 'cleanup-index-remove'}catch{$report.cleanup_index_error=$_.Exception.Message;$final='failed owned index teardown'}}
 if($install){$left=@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath-and$_.ExecutablePath.StartsWith($install,[StringComparison]::OrdinalIgnoreCase)});$report.remaining_owned_installed_processes=$left.Count;if($left.Count){$final='failed owned process teardown'}}
 $report.status=$final;Save
}
if(-not$final.StartsWith('pass')){exit 1}
