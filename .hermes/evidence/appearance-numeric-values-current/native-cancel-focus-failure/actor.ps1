param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExpectedSid)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$out=Join-Path $Root 'results';$report=@{status='running';scope='Exact current installer at actual Windows144DPI; named visible numeric editing/restore/Cancel, all six native window types and complete work-area capture containment; live owners use complete native-region observation while editor owns input, with separate unobscured saved-startup captures; Native/OpenGL';cases=@();numeric_actions=@();gui_idle_baseline=@()};$final='failed native DPI/layout audit';$owned=[Collections.Generic.List[Diagnostics.Process]]::new();$install=$null
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
 Add-Type -ReferencedAssemblies System.Windows.Forms,System.Drawing @'
using System;using System.Collections.Generic;using System.Runtime.InteropServices;using System.Threading;using System.Drawing;using System.Windows.Forms;
public static class DpiDesktop {
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)]struct Mode {
  [MarshalAs(UnmanagedType.ByValTStr,SizeConst=32)]public string device;
  public ushort spec,driver,size,extra;public uint fields;public int x,y;public uint orientation,fixedOutput;public short color,duplex,yResolution,ttOption,collate;
  [MarshalAs(UnmanagedType.ByValTStr,SizeConst=32)]public string form;
  public ushort logPixels;public uint bits,width,height,flags,frequency,icmMethod,icmIntent,media,dither,reserved1,reserved2,panWidth,panHeight;
 }
 [StructLayout(LayoutKind.Sequential)]struct Rect {public int left,top,right,bottom;}
 [StructLayout(LayoutKind.Sequential)]struct MonitorInfo {public int size;public Rect monitor,work;public uint flags;}
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern bool EnumDisplaySettingsW(string name,int mode,ref Mode data);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern int ChangeDisplaySettingsExW(string name,ref Mode mode,IntPtr hwnd,uint flags,IntPtr parameter);
 [DllImport("user32.dll")]public static extern uint GetDpiForWindow(IntPtr window);
 [DllImport("user32.dll")]static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 [DllImport("user32.dll")]static extern bool GetWindowRect(IntPtr window,out Rect rect);
 [DllImport("user32.dll")]static extern IntPtr MonitorFromWindow(IntPtr window,uint flags);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)]static extern bool GetMonitorInfoW(IntPtr monitor,ref MonitorInfo info);
 [DllImport("user32.dll")]static extern void mouse_event(uint flags,uint x,uint y,uint data,UIntPtr extra);
 public static string[] Modes(){var modes=new List<string>();for(int i=0;i<1024;i++){Mode m=new Mode();m.size=220;if(!EnumDisplaySettingsW(null,i,ref m))break;string v=m.width+"x"+m.height+"x"+m.bits;if(!modes.Contains(v))modes.Add(v);}return modes.ToArray();}
 public static string LargeMode(){
  if(Marshal.SizeOf(typeof(Mode))!=220)throw new Exception("Unexpected native DEVMODEW size");
  Mode best=new Mode();ulong area=ulong.MaxValue;
  for(int i=0;i<1024;i++){Mode mode=new Mode();mode.size=220;if(!EnumDisplaySettingsW(null,i,ref mode))break;ulong a=(ulong)mode.width*mode.height;if(mode.bits>=32&&mode.width>=1920&&mode.height>=1440&&a<area){best=mode;area=a;}}
  if(area==ulong.MaxValue)throw new Exception("Driver has no supported1920x1440-or-larger mode");
  int result=ChangeDisplaySettingsExW(null,ref best,IntPtr.Zero,0,IntPtr.Zero);if(result!=0)throw new Exception("Native display change failed: "+result);Thread.Sleep(1500);return best.width+"x"+best.height;
 }
 [DllImport("user32.dll")]public static extern bool IsWindowVisible(IntPtr window);
 [DllImport("user32.dll")]static extern bool PostMessageW(IntPtr window,uint message,IntPtr wparam,IntPtr lparam);
 public static void Close(IntPtr window){if(!PostMessageW(window,0x10,IntPtr.Zero,IntPtr.Zero))throw new Exception("Normal Settings close request failed");}
 public static void PerMonitor(){if(SetThreadDpiAwarenessContext(new IntPtr(-4))==IntPtr.Zero)throw new Exception("Cannot enter native per-monitor V2 observation context");}
 public static int[] Bounds(IntPtr window){Rect r;if(!GetWindowRect(window,out r))throw new Exception("Window bounds unavailable");return new int[]{r.left,r.top,r.right-r.left,r.bottom-r.top};}
 public static int[] Work(IntPtr window){MonitorInfo m=new MonitorInfo();m.size=Marshal.SizeOf(typeof(MonitorInfo));if(!GetMonitorInfoW(MonitorFromWindow(window,2),ref m))throw new Exception("Work area unavailable");return new int[]{m.work.left,m.work.top,m.work.right-m.work.left,m.work.bottom-m.work.top};}
 public static bool Fits(IntPtr window){int[] b=Bounds(window),a=Work(window);return b[0]>=a[0]&&b[1]>=a[1]&&b[0]+b[2]<=a[0]+a[2]&&b[1]+b[3]<=a[1]+a[3];}
 public static void WheelAt(int x,int y,int delta){Cursor.Position=new Point(x,y);mouse_event(0x800,0,0,unchecked((uint)delta),UIntPtr.Zero);Thread.Sleep(200);}
}

'@
 function Await($pidValue,$title){$deadline=[DateTime]::UtcNow.AddSeconds(25);do{$v=[PickerNative]::Window($pidValue,$title);if($v-ne[IntPtr]::Zero){return $v};Pump}while([DateTime]::UtcNow-lt$deadline);throw ('Missing owned window '+$title)}



 function ObserveWindow($handle,$label,$radius,[bool]$capture){
  [PickerNative]::Place($handle);$enabled=[PickerNative]::IsWindowEnabled($handle);$file=$null
  if($capture){[PickerNative]::Focus($handle);$position=[DpiDesktop]::Bounds($handle);[Windows.Forms.Cursor]::Position=[Drawing.Point]::new($position[0]+80,$position[1]+15)}
  Start-Sleep -Milliseconds 700;$dpi=[DpiDesktop]::GetDpiForWindow($handle);$physical=[int]($radius*$dpi/96);$matches=[RadiusProbe]::Matches($handle,$physical);$fits=[DpiDesktop]::Fits($handle)
  if($capture){$file=$renderer+'-'+$label+'-'+$radius+'.png';[PickerNative]::Shot($handle,(Join-Path $out $file))}
  $report.cases+=@{renderer=$renderer;case=$label;logical_radius=$radius;dpi=$dpi;physical_radius=$physical;native_expected_region_match=$matches;window_fits_work_area=$fits;bounds=[DpiDesktop]::Bounds($handle);work_area=[DpiDesktop]::Work($handle);capture=$file;foreground_capture_requested=$capture;window_enabled=$enabled};Save
  Require ($dpi-eq144) ('Expected actual144DPI, got '+$dpi);Require $matches ('DPI-scaled native region mismatch '+$label);Require $fits ('Window extends outside native work area '+$label)
 }

 function CloseOwned($handle){
  Invoke $handle 'Close window';$deadline=[DateTime]::UtcNow.AddSeconds(15)
  do{if(-not[DpiDesktop]::IsWindowVisible($handle)){return};Pump}while([DateTime]::UtcNow-lt$deadline)
  [PickerNative]::Shot($handle,(Join-Path $out 'close-not-completed.png'));throw 'Invoked Close window did not close its native viewport'
 }
 function CaptureLive($handle,$label,$radius){ObserveWindow $handle $label $radius $true}
 function ObserveOwner($handle,$label,$radius){ObserveWindow $handle $label $radius $false}
 function NativePath($path){return @{encoding='windows-utf16-le';units_hex=(-join $path.ToCharArray().ForEach({([int]$_).ToString('x4')}))}}

 function SetCorners($editor,$value){
  [PickerNative]::Focus($editor);$window=[DpiDesktop]::Bounds($editor);$visible=$false
  for($scroll=0;$scroll-lt16;$scroll++){
   $numeric=Node $editor 'Window corners' ([Windows.Automation.ControlType]::Spinner);$bounds=$numeric.Current.BoundingRectangle
   if($bounds.Top-gt$window[1]+120-and$bounds.Bottom-lt$window[1]+$window[3]-100){$visible=$true;break}
   [DpiDesktop]::WheelAt([int]($bounds.Left+20),[int]($window[1]+$window[3]/2),-480);Pump
  }
  Require $visible 'Named Window corners control is not visible inside the property viewport'
  $numeric=Node $editor 'Window corners' ([Windows.Automation.ControlType]::Spinner);$before=$numeric.GetCurrentPattern([Windows.Automation.RangeValuePattern]::Pattern).Current.Value;$numeric.SetFocus();$deadline=[DateTime]::UtcNow.AddSeconds(5)
  do{$numeric=Node $editor 'Window corners' ([Windows.Automation.ControlType]::Spinner);if($numeric.Current.HasKeyboardFocus){break};Pump}while([DateTime]::UtcNow-lt$deadline)
  $focused=$numeric.Current.HasKeyboardFocus;Require $focused 'Named numeric control did not acknowledge keyboard focus before typing'
  [Windows.Forms.SendKeys]::SendWait('^a');[Windows.Forms.SendKeys]::SendWait([string]$value);[Windows.Forms.SendKeys]::SendWait('{ENTER}');AwaitCorners $editor $value 'typed commit'
  $report.numeric_actions+=@{action='typed commit';before=$before;expected=$value;keyboard_focus_acknowledged=$focused};Save
 }
 function AwaitCorners($editor,$value,$action){
  $deadline=[DateTime]::UtcNow.AddSeconds(5)
  do{$actual=(Node $editor 'Window corners' ([Windows.Automation.ControlType]::Spinner)).GetCurrentPattern([Windows.Automation.RangeValuePattern]::Pattern).Current.Value;if($actual-eq$value){return};Pump}while([DateTime]::UtcNow-lt$deadline)
  $focus=[Windows.Automation.AutomationElement]::FocusedElement;$report.numeric_failure=@{action=$action;expected=$value;actual=$actual;focused_name=$focus.Current.Name;focused_type=$focus.Current.ControlType.ProgrammaticName};[PickerNative]::Shot($editor,(Join-Path $out 'numeric-mismatch.png'));Save;throw ('Named native value mismatch after '+$action+': expected='+$value+' actual='+$actual)

 }

 function MeasureGuiIdle($process,$window){
  Phase ($renderer+' separate GUI idle baseline');$deadline=[DateTime]::UtcNow.AddSeconds(60);$attempt=0
  do{$attempt++;$state=Run $app 'index status --json' ('idle-index-'+$renderer+'-'+$attempt);$index=$state.stdout|ConvertFrom-Json;if($index.status-eq'ready'-and$index.watcher_ready){break};Start-Sleep -Milliseconds 500}while([DateTime]::UtcNow-lt$deadline)
  if($index.status-ne'ready'-or-not$index.watcher_ready){$report.gui_idle_baseline+=@{renderer=$renderer;status='unmeasured: index not ready';index=$index};Save;return}
  [PickerNative]::Place($window);[PickerNative]::Focus($window);$area=[DpiDesktop]::Work($window);[Windows.Forms.Cursor]::Position=[Drawing.Point]::new($area[2]-2,$area[3]-2);Start-Sleep -Seconds 5
  $process.Refresh();$before=$process.TotalProcessorTime.TotalSeconds;$rssBefore=$process.WorkingSet64;$gdiBefore=[RadiusProbe]::GetGuiResources($process.Handle,0);$clock=[Diagnostics.Stopwatch]::StartNew();Start-Sleep -Seconds 30;$process.Refresh();$seconds=$clock.Elapsed.TotalSeconds;$cpu=100*($process.TotalProcessorTime.TotalSeconds-$before)/$seconds;$rssAfter=$process.WorkingSet64;$gdiAfter=[RadiusProbe]::GetGuiResources($process.Handle,0)
  $after=(Run $app 'index status --json' ('idle-index-'+$renderer+'-after')).stdout|ConvertFrom-Json;$idle=$after.status-eq'ready'-and$after.watcher_ready
  $report.gui_idle_baseline+=@{renderer=$renderer;status=if(-not$idle){'unverified: index state changed'}elseif($cpu-lt2){'pass current-scope criterion'}else{'failed current-scope criterion'};criterion='StaticGUIbelow2percentonecore';cpu_one_core_percent=$cpu;seconds=$seconds;dpi=[DpiDesktop]::GetDpiForWindow($window);bounds=[DpiDesktop]::Bounds($window);work_area=$area;rss_before=$rssBefore;rss_after=$rssAfter;gdi_before=$gdiBefore;gdi_after=$gdiAfter;index_before=$index;index_after=$after;scope='Same signed VirtIO/software fixture and exact current package; Dark144DPI, initial main radius0. GUI process only; no daemon CPU sample, physical or cross-source comparison.'};Save
 }
 Phase 'prepare native large display';$report.display_modes=[DpiDesktop]::Modes();Save;$report.display_mode=[DpiDesktop]::LargeMode();Save

 $null=Start-Process 'ms-settings:display';$deadline=[DateTime]::UtcNow.AddSeconds(90);$systemWindow=[IntPtr]::Zero;$settingsCandidates=@();$actorSession=[Diagnostics.Process]::GetCurrentProcess().SessionId
 do{
  $hosts=@(Get-Process SystemSettings,ApplicationFrameHost -ErrorAction SilentlyContinue|Where-Object {$_.SessionId-eq$actorSession});$ids=@($hosts|ForEach-Object {$_.Id});$nameCondition=[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,'Settings')
  $settingsCandidates=@([Windows.Automation.AutomationElement]::RootElement.FindAll([Windows.Automation.TreeScope]::Children,$nameCondition)|Where-Object {$_.Current.ProcessId-in$ids-and-not$_.Current.IsOffscreen-and$_.Current.NativeWindowHandle-ne0})
  if($settingsCandidates.Count-eq1){$systemWindow=[IntPtr]::new([int64]$settingsCandidates[0].Current.NativeWindowHandle);$report.settings_host=@{pid=$settingsCandidates[0].Current.ProcessId;type=$settingsCandidates[0].Current.ControlType.ProgrammaticName;title=$settingsCandidates[0].Current.Name};Save;break};Pump
 }while([DateTime]::UtcNow-lt$deadline)
 if($systemWindow-eq[IntPtr]::Zero){$report.settings_discovery=@{hosts=@($hosts|Select-Object Name,Id,SessionId);visible_top_windows=@([Windows.Automation.AutomationElement]::RootElement.FindAll([Windows.Automation.TreeScope]::Children,[Windows.Automation.Condition]::TrueCondition)|ForEach-Object {@{name=$_.Current.Name;pid=$_.Current.ProcessId;type=$_.Current.ControlType.ProgrammaticName;native_handle=$_.Current.NativeWindowHandle}})};Save}
 Require ($systemWindow-ne[IntPtr]::Zero) 'Native Windows Display Settings did not open';[PickerNative]::Place($systemWindow);[PickerNative]::Focus($systemWindow);[PickerNative]::Shot($systemWindow,(Join-Path $out 'windows-display-settings.png'))
 $nodes=@((Tree $systemWindow).FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)|Where-Object {-not$_.Current.IsOffscreen}|ForEach-Object {@{name=$_.Current.Name;type=$_.Current.ControlType.ProgrammaticName;bounds=$_.Current.BoundingRectangle.ToString()}});[IO.File]::WriteAllText((Join-Path $out 'windows-display-tree.json'),($nodes|ConvertTo-Json -Depth 6),[Text.UTF8Encoding]::new($false))

 Phase 'select normal Windows150percent Scale';$scaleCombo=Node $systemWindow 'Scale' ([Windows.Automation.ControlType]::ComboBox);$settingsPid=$scaleCombo.Current.ProcessId;$scaleCombo.GetCurrentPattern([Windows.Automation.ExpandCollapsePattern]::Pattern).Expand();Pump
 $deadline=[DateTime]::UtcNow.AddSeconds(20);$choice=@()
 do{$condition=[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,'150%');$choice=@([Windows.Automation.AutomationElement]::RootElement.FindAll([Windows.Automation.TreeScope]::Descendants,$condition)|Where-Object {$_.Current.ProcessId-eq$settingsPid-and$_.Current.ControlType-eq[Windows.Automation.ControlType]::ListItem-and-not$_.Current.IsOffscreen});if($choice.Count-ne1){Pump}}while($choice.Count-ne1-and[DateTime]::UtcNow-lt$deadline)
 Require ($choice.Count-eq1) 'Native150percent option not uniquely visible';$choice[0].GetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern).Select();$deadline=[DateTime]::UtcNow.AddSeconds(20)
 while([DpiDesktop]::GetDpiForWindow($systemWindow)-ne144){Require ([DateTime]::UtcNow-lt$deadline) 'Native Settings did not reach144DPI';Pump}
 [DpiDesktop]::PerMonitor();[PickerNative]::Shot($systemWindow,(Join-Path $out 'windows-display-settings-150.png'));$report.dpi_setup=@{method='Native Scale ComboBox Expand, visible150percent ListItem Select';actual_settings_dpi=[DpiDesktop]::GetDpiForWindow($systemWindow);selected_label='150%'};Save
 [DpiDesktop]::Close($systemWindow);Start-Sleep -Milliseconds 500
 foreach($renderer in @('Native','OpenGL')){
  Phase ($renderer+' live editor radius');$config.theme='Dark';$config.window_size=@(1000,700);$config.graphics_renderer=if($renderer-eq'Native'){'Native'}else{'OpenGlCompatibility'};$config.active_custom_theme='Radius audit';$config.custom_themes=@(@{name='Radius audit';base_preset='Ottrin';dark=@{app_radius=0};light=@{app_radius=0}});$rendererArgs=if($renderer-eq'OpenGL'){' --renderer opengl'}else{''};[IO.File]::WriteAllText($pref,($config|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false))
  $s=[Diagnostics.ProcessStartInfo]::new($app,('"'+$corpus+'"'+$rendererArgs));$s.UseShellExecute=$false;$p=[Diagnostics.Process]::Start($s);$owned.Add($p);$w=Await $p.Id 'Ottrin';$null=Node $w 'Menu' ([Windows.Automation.ControlType]::Button)
  MeasureGuiIdle $p $w
  Invoke $w 'Menu';Invoke $w ('Settings'+[char]0x2026);$settings=Await $p.Id 'Ottrin Settings'
  (Node $settings 'Appearance' ([Windows.Automation.ControlType]::RadioButton)).GetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern).Select();Pump
  $buttons=@();for($i=0;$i-lt12;$i++){$buttons=@(Nodes $settings 'Edit Radius audit'|Where-Object {$_.Current.ControlType-eq[Windows.Automation.ControlType]::Button});if($buttons.Count-eq1){break};[PickerNative]::Wheel($settings,-480);Pump};Require ($buttons.Count-eq1) 'Saved theme edit not visible';$buttons[0].GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke();$editor=Await $p.Id 'Ottrin Theme Editor';[RadiusProbe]::Resize($editor,1770,1140);Pump
  foreach($radius in @(24,0)){SetCorners $editor $radius;CaptureLive $editor 'editor-live' $radius;ObserveOwner $settings 'settings-live' $radius;ObserveOwner $w 'main-live' $radius}
  SetCorners $editor 24;Invoke $editor 'Restore Window corners';Start-Sleep -Milliseconds 700;AwaitCorners $editor 0 'named restore';CaptureLive $editor 'editor-after-restore' 0;ObserveOwner $settings 'settings-after-restore' 0;ObserveOwner $w 'main-after-restore' 0
  SetCorners $editor 24;CaptureLive $editor 'editor-before-cancel' 24;Invoke $editor 'Cancel';Start-Sleep -Milliseconds 700;CaptureLive $settings 'settings-after-cancel' 0
  CloseOwned $settings;CaptureLive $w 'main-after-cancel' 0;Invoke $w 'Close window';Require ($p.WaitForExit(15000)-and$p.ExitCode-eq0) 'Normal close failed';$savedConfig=Get-Content $pref -Raw|ConvertFrom-Json;Require ($savedConfig.custom_themes[0].dark.app_radius-eq0) 'Cancel persisted unsaved radius';Require ((Get-FileHash $source).Hash-eq$sourceHash) 'Source changed';Save
 }

 foreach($renderer in @('Native','OpenGL')){
  Phase ($renderer+'144DPI other native windows');$config.graphics_renderer=if($renderer-eq'Native'){'Native'}else{'OpenGlCompatibility'};$config.custom_themes[0].dark.app_radius=24;$config.custom_themes[0].light.app_radius=24;$rendererArgs=if($renderer-eq'OpenGL'){' --renderer opengl'}else{''};[IO.File]::WriteAllText($pref,($config|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false))
  $s=[Diagnostics.ProcessStartInfo]::new($app,('"'+$corpus+'"'+$rendererArgs));$s.UseShellExecute=$false;$p=[Diagnostics.Process]::Start($s);$owned.Add($p);$w=Await $p.Id 'Ottrin';CaptureLive $w 'main-saved-startup' 24;Invoke $w 'Menu';Invoke $w ('Settings'+[char]0x2026);$settings=Await $p.Id 'Ottrin Settings';CaptureLive $settings 'settings-saved-startup' 24;CloseOwned $settings;$row=Node $w 'preserve.txt' ([Windows.Automation.ControlType]::Button);$toggle=$row.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern);if($toggle.Current.ToggleState-ne[Windows.Automation.ToggleState]::On){$toggle.Toggle()};Start-Sleep -Milliseconds 500
  Invoke $w 'Menu';Invoke $w 'Quick Inspector Selection';$inspector=Await $p.Id ('Ottrin Quick Inspector '+[char]0x2014+' preserve.txt');CaptureLive $inspector 'inspector-scaled' 24;Invoke $inspector 'Close window';Invoke $w 'Close window';Require ($p.WaitForExit(15000)-and$p.ExitCode-eq0) 'Normal main close failed'
  $s=[Diagnostics.ProcessStartInfo]::new($app,('picker'+$rendererArgs));$s.UseShellExecute=$false;$s.RedirectStandardInput=$true;$s.RedirectStandardOutput=$true;$s.RedirectStandardError=$true;$picker=[Diagnostics.Process]::Start($s);$owned.Add($picker);$stdout=$picker.StandardOutput.ReadToEndAsync();$stderr=$picker.StandardError.ReadToEndAsync();$picker.StandardInput.Write((@{mode='open';title='DPI probe';initial_directory=(NativePath $corpus)}|ConvertTo-Json -Depth 5 -Compress));$picker.StandardInput.Close();$pickWindow=Await $picker.Id ('Ottrin '+[char]0x2014+' DPI probe');CaptureLive $pickWindow 'picker-scaled' 24;Invoke $pickWindow 'Cancel';Require ($picker.WaitForExit(15000)-and$picker.ExitCode-eq0) 'Picker cancel failed';Require ($stdout.Wait(5000)-and$stderr.Wait(5000)) 'Picker streams incomplete';Require (-not($stdout.Result|ConvertFrom-Json).accepted) 'Picker cancel accepted'
  $s=[Diagnostics.ProcessStartInfo]::new($app,('--properties "'+$source+'"'+$rendererArgs));$s.UseShellExecute=$false;$properties=[Diagnostics.Process]::Start($s);$owned.Add($properties);$propWindow=Await $properties.Id ('Ottrin '+[char]0x2014+' Properties '+[char]0x2014+' preserve.txt');CaptureLive $propWindow 'properties-scaled' 24;Invoke $propWindow 'Close window';Require ($properties.WaitForExit(15000)-and$properties.ExitCode-eq0) 'Properties close failed'
 }
 $saved=(Get-FileHash $pref).Hash;Phase 'supported uninstall';$uninstaller=Join-Path $out 'owned-uninstall.exe';Copy-Item (Join-Path $install 'Uninstall.exe') $uninstaller;$null=Run $uninstaller ('/S _?='+$install) 'uninstall';Remove-Item $uninstaller;Require (-not(Test-Path $install)) 'Installation remains';Require ((Get-FileHash $pref).Hash-eq$saved) 'Uninstall changed preferences';Require ((Get-FileHash $source).Hash-eq$sourceHash) 'Source changed';$report.preferences_preserved=$true;$report.source_preserved=$true;$final='pass native144DPI shape/visibility/live editing; originalsrequire review'
}catch{$report.error=$_.Exception.Message;$report.stack=$_.ScriptStackTrace}
finally{
 if($picker){try{if($picker.HasExited-and$stdout-and$stderr){$null=$stdout.Wait(5000);$null=$stderr.Wait(5000);$report.picker_child=@{exit=$picker.ExitCode;stdout=$stdout.Result;stderr=$stderr.Result}}}catch{$report.picker_diagnostic_error=$_.Exception.Message}}
 foreach($p in $owned){if(-not$p.HasExited){& "$env:SystemRoot\System32\taskkill.exe" /PID $p.Id /T /F|Out-Null;$null=$p.WaitForExit(10000)}}
 if($install-and(Test-Path $app)-and$manifest-and(Get-FileHash $app).Hash.ToLowerInvariant()-eq$manifest.'bin/ottrin.exe'){try{$report.cleanup_index_remove=Run $app 'index service-remove' 'cleanup-index-remove'}catch{$report.cleanup_index_error=$_.Exception.Message;$final='failed owned index teardown'}}
 if($install){$left=@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath-and$_.ExecutablePath.StartsWith($install,[StringComparison]::OrdinalIgnoreCase)});$report.remaining_owned_installed_processes=$left.Count;if($left.Count){$final='failed owned process teardown'}}
 $report.status=$final;Save
}
if(-not$final.StartsWith('pass')){exit 1}
