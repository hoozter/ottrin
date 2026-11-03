param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExpectedSid)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$out=Join-Path $Root 'results';$report=@{status='running';named_fields=@();scope='Exact installed native Windows region shape across main Settings editor Inspector picker Properties,0/24radius,Native/OpenGL,resize/maximize andboundedidle; source/preferences preservation';cases=@();idle=@()};$final='failed native main field audit';$owned=[Collections.Generic.List[Diagnostics.Process]]::new();$forms=[Collections.Generic.List[object]]::new();$install=$null
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
function Field($w,$name){$n=Node $w $name ([Windows.Automation.ControlType]::Edit);$v=$n.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern).Current;return @{name=$n.Current.Name;value=$v.Value;read_only=$v.IsReadOnly;role='Edit';height=$n.Current.BoundingRectangle.Height;width=$n.Current.BoundingRectangle.Width}}
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
[DllImport("user32.dll")]static extern int GetWindowRgn(IntPtr w,IntPtr r);
[DllImport("user32.dll")]static extern bool GetWindowRect(IntPtr w,out Rect r);
[DllImport("gdi32.dll")]static extern bool PtInRegion(IntPtr r,int x,int y);
[DllImport("gdi32.dll")]static extern bool DeleteObject(IntPtr r);
[DllImport("user32.dll")]static extern bool SetWindowPos(IntPtr h,IntPtr after,int x,int y,int width,int height,uint flags);
[DllImport("user32.dll")]public static extern bool IsZoomed(IntPtr h);
[DllImport("user32.dll")]public static extern uint GetGuiResources(IntPtr h,uint flags);
public static void Resize(IntPtr h,int width,int height){if(!SetWindowPos(h,IntPtr.Zero,0,0,width,height,0x16))throw new Exception("Owned window resize failed");}
public static string Region(IntPtr w){var r=CreateRectRgn(0,0,0,0);if(r==IntPtr.Zero)throw new Exception("Query region allocation failed");try{Rect bounds;if(!GetWindowRect(w,out bounds))throw new Exception("No owned window bounds");int kind=GetWindowRgn(w,r);int width=bounds.right-bounds.left,height=bounds.bottom-bounds.top;return "kind="+kind+" topLeft="+PtInRegion(r,1,1)+" topRight="+PtInRegion(r,width-2,1)+" bottomLeft="+PtInRegion(r,1,height-2)+" bottomRight="+PtInRegion(r,width-2,height-2)+" center="+PtInRegion(r,width/2,height/2)+" dimensions="+width+"x"+height;}finally{DeleteObject(r);}}
}
'@
 function NativePath($path){return @{encoding='windows-utf16-le';units_hex=(-join $path.ToCharArray().ForEach({([int]$_).ToString('x4')}))}}
 function Await($pidValue,$title){$deadline=[DateTime]::UtcNow.AddSeconds(25);do{$v=[PickerNative]::Window($pidValue,$title);if($v-ne[IntPtr]::Zero){return $v};Pump}while([DateTime]::UtcNow-lt$deadline);throw ('Missing owned window '+$title)}
 function Capture($handle,$label,$radius,$place=$true){if($place){[PickerNative]::Place($handle)};[PickerNative]::Focus($handle);Start-Sleep -Milliseconds 700;$native=[RadiusProbe]::Region($handle);$name=($renderer+'-'+$label+'-'+$radius+'.png');[PickerNative]::Shot($handle,(Join-Path $out $name));$report.cases+=@{case=$label;renderer=$renderer;radius=$radius;geometry=[PickerNative]::Geometry($handle);native_region=$native;capture=$name};Save;if($radius-eq0){Require ($native.StartsWith('kind=2 ')-and$native.Contains('topLeft=True')-and$native.Contains('bottomRight=True')) ('Expected rectangular native region: '+$native)}else{Require ($native.StartsWith('kind=3 ')-and$native.Contains('topLeft=False')-and$native.Contains('topRight=False')-and$native.Contains('bottomLeft=False')-and$native.Contains('bottomRight=False')) ('Expected rounded native region: '+$native)};Require ($native.Contains('center=True')) 'Window center excluded'}
 foreach($renderer in @('Native','OpenGL')){foreach($radius in @(0,24)){
  Phase ($renderer+' radius '+$radius);$config.theme='Dark';$config.graphics_renderer=if($renderer-eq'Native'){'Native'}else{'OpenGlCompatibility'};$config.active_custom_theme='Radius audit';$config.custom_themes=@(@{name='Radius audit';base_preset='Ottrin';dark=@{app_radius=$radius};light=@{app_radius=$radius}});$rendererArgs=if($renderer-eq'OpenGL'){' --renderer opengl'}else{''}
  [IO.File]::WriteAllText($pref,($config|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false))
  $s=[Diagnostics.ProcessStartInfo]::new($app,('"'+$corpus+'"'+$rendererArgs));$s.UseShellExecute=$false;$p=[Diagnostics.Process]::Start($s);$owned.Add($p);$w=Await $p.Id 'Ottrin';Capture $w 'main' $radius
  [RadiusProbe]::Resize($w,1000,700);Pump;Capture $w 'main-resized' $radius
  Invoke $w 'Maximize or restore window';$deadline=[DateTime]::UtcNow.AddSeconds(10);while(-not[RadiusProbe]::IsZoomed($w)){Require ([DateTime]::UtcNow-lt$deadline) 'Maximize did not complete';Pump};Capture $w 'main-maximized' 0 $false;Invoke $w 'Maximize or restore window';$deadline=[DateTime]::UtcNow.AddSeconds(10);while([RadiusProbe]::IsZoomed($w)){Require ([DateTime]::UtcNow-lt$deadline) 'Restore did not complete';Pump};Capture $w 'main-restored' $radius
  Invoke $w 'Menu';Invoke $w ('Settings'+[char]0x2026);$settings=Await $p.Id 'Ottrin Settings';Capture $settings 'settings' $radius
  (Node $settings 'Appearance' ([Windows.Automation.ControlType]::RadioButton)).GetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern).Select();Pump
  $editorButtons=@();for($i=0;$i-lt12;$i++){$editorButtons=@(Nodes $settings 'Edit Radius audit'|Where-Object {$_.Current.ControlType-eq[Windows.Automation.ControlType]::Button});if($editorButtons.Count-eq1){break};[PickerNative]::Wheel($settings,-480);Pump};Require ($editorButtons.Count-eq1) 'Saved theme edit action not visible';$editorButtons[0].GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke();$editor=Await $p.Id 'Ottrin Theme Editor';[RadiusProbe]::Resize($editor,1180,720);Pump;Capture $editor 'editor' $radius;Invoke $editor 'Cancel';Start-Sleep -Milliseconds 500;Invoke $settings 'Close window';Start-Sleep -Milliseconds 500
  $row=Node $w 'preserve.txt' ([Windows.Automation.ControlType]::Button);$toggle=$row.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern);if($toggle.Current.ToggleState-ne[Windows.Automation.ToggleState]::On){$toggle.Toggle()};$deadline=[DateTime]::UtcNow.AddSeconds(10);do{Pump;$row=Node $w 'preserve.txt' ([Windows.Automation.ControlType]::Button);$state=$row.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern).Current.ToggleState}while($state-ne[Windows.Automation.ToggleState]::On-and[DateTime]::UtcNow-lt$deadline);Require ($state-eq[Windows.Automation.ToggleState]::On) 'File selection failed'
  Invoke $w 'Menu';Invoke $w 'Quick Inspector Selection';$inspector=Await $p.Id ('Ottrin Quick Inspector '+[char]0x2014+' preserve.txt');Capture $inspector 'inspector' $radius;Invoke $inspector 'Close window';Start-Sleep -Milliseconds 500
  if($radius-eq24){$p.Refresh();$before=$p.TotalProcessorTime.TotalSeconds;$gdiBefore=[RadiusProbe]::GetGuiResources($p.Handle,0);$clock=[Diagnostics.Stopwatch]::StartNew();Start-Sleep -Seconds 30;$p.Refresh();$cpu=100*($p.TotalProcessorTime.TotalSeconds-$before)/$clock.Elapsed.TotalSeconds;$gdiAfter=[RadiusProbe]::GetGuiResources($p.Handle,0);$report.idle+=@{renderer=$renderer;seconds=$clock.Elapsed.TotalSeconds;cpu_one_core_percent=$cpu;gdi_before=$gdiBefore;gdi_after=$gdiAfter};Save;Require ($cpu-lt2) 'Static main window busy after region update; investigate before delivery'}
  Invoke $w 'Close window';Require ($p.WaitForExit(15000)-and$p.ExitCode-eq0) 'Native main close failed';Require ((Get-FileHash $source).Hash-eq$sourceHash) 'Source changed'
  $s=[Diagnostics.ProcessStartInfo]::new($app,('picker'+$rendererArgs));$s.UseShellExecute=$false;$s.RedirectStandardInput=$true;$s.RedirectStandardOutput=$true;$s.RedirectStandardError=$true;$picker=[Diagnostics.Process]::Start($s);$owned.Add($picker);$stdout=$picker.StandardOutput.ReadToEndAsync();$stderr=$picker.StandardError.ReadToEndAsync();$picker.StandardInput.Write((@{mode='open';title='Radius probe';initial_directory=(NativePath $corpus)}|ConvertTo-Json -Depth 5 -Compress));$picker.StandardInput.Close();$pickWindow=Await $picker.Id ('Ottrin '+[char]0x2014+' Radius probe');Capture $pickWindow 'picker' $radius;Invoke $pickWindow 'Cancel';Require ($picker.WaitForExit(15000)-and$picker.ExitCode-eq0) 'Picker cancel failed';Require ($stdout.Wait(5000)-and$stderr.Wait(5000)) 'Picker streams incomplete';Require (-not($stdout.Result|ConvertFrom-Json).accepted) 'Picker cancel accepted a file'
  $s=[Diagnostics.ProcessStartInfo]::new($app,('--properties "'+$source+'"'+$rendererArgs));$s.UseShellExecute=$false;$properties=[Diagnostics.Process]::Start($s);$owned.Add($properties);$propWindow=Await $properties.Id ('Ottrin '+[char]0x2014+' Properties '+[char]0x2014+' preserve.txt');Capture $propWindow 'properties' $radius;Invoke $propWindow 'Close window';Require ($properties.WaitForExit(15000)-and$properties.ExitCode-eq0) 'Properties close failed'
  $savedConfig=Get-Content $pref -Raw|ConvertFrom-Json;Require ($savedConfig.custom_themes[0].dark.app_radius-eq$radius) 'Configured radius changed';Save
 }}
 $saved=(Get-FileHash $pref).Hash;Phase 'supported uninstall';$uninstaller=Join-Path $out 'owned-uninstall.exe';Copy-Item (Join-Path $install 'Uninstall.exe') $uninstaller;$null=Run $uninstaller ('/S _?='+$install) 'uninstall';Remove-Item $uninstaller;Require (-not(Test-Path $install)) 'Installation remains';Require ((Get-FileHash $pref).Hash-eq$saved) 'Uninstall changed preferences';Require ((Get-FileHash $source).Hash-eq$sourceHash) 'Source changed';$report.preferences_preserved=$true;$report.source_preserved=$true;$final='pass native installed window shape checks; original captures require review'
}catch{$report.error=$_.Exception.Message;$report.stack=$_.ScriptStackTrace}
finally{
 if($picker){try{if($picker.HasExited-and$stdout-and$stderr){$null=$stdout.Wait(5000);$null=$stderr.Wait(5000);$report.picker_child=@{exit=$picker.ExitCode;stdout=$stdout.Result;stderr=$stderr.Result}}}catch{$report.picker_diagnostic_error=$_.Exception.Message}}
 foreach($p in $owned){if(-not$p.HasExited){& "$env:SystemRoot\System32\taskkill.exe" /PID $p.Id /T /F|Out-Null;$null=$p.WaitForExit(10000)}}
 if($install-and(Test-Path $app)-and$manifest-and(Get-FileHash $app).Hash.ToLowerInvariant()-eq$manifest.'bin/ottrin.exe'){try{$report.cleanup_index_remove=Run $app 'index service-remove' 'cleanup-index-remove'}catch{$report.cleanup_index_error=$_.Exception.Message;$final='failed owned index teardown'}}
 if($install){$left=@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath-and$_.ExecutablePath.StartsWith($install,[StringComparison]::OrdinalIgnoreCase)});$report.remaining_owned_installed_processes=$left.Count;if($left.Count){$final='failed owned process teardown'}}
 $report.status=$final;Save
}
if(-not$final.StartsWith('pass')){exit 1}
