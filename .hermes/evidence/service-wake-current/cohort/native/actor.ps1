param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExpectedSid)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$out=Join-Path $Root 'results';$report=@{status='running';scope='Exact current NSIS/ordinary user, Dark144DPI Native and OpenGL; malformed settings refusal, retained native theme draft after save failure, repaired retry and truthful Search health. Other appearance/MSIX/physical scopes separate';cases=@()};$final='failed native settings preservation workflow';$readonlyConfig=$null;$originalAttributes=$null;$owned=[Collections.Generic.List[Diagnostics.Process]]::new();$install=$null
function Require($value,$message){if(-not$value){throw $message}}
function Save{$p=Join-Path $out 'RESULT.json';$tmp=$p+'.next';[IO.File]::WriteAllText($tmp,($report|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false));if(Test-Path $p){[IO.File]::Replace($tmp,$p,[NullString]::Value)}else{[IO.File]::Move($tmp,$p)}}
function Phase($value){$report.phase=$value;Save}
function Run($exe,$arguments,$name,$expectedExit=0){
 $s=[Diagnostics.ProcessStartInfo]::new($exe,$arguments);$s.UseShellExecute=$false;$s.CreateNoWindow=$true;$s.RedirectStandardOutput=$true;$s.RedirectStandardError=$true;$s.StandardOutputEncoding=[Text.UTF8Encoding]::new($false);$s.StandardErrorEncoding=[Text.UTF8Encoding]::new($false);$clock=[Diagnostics.Stopwatch]::StartNew();$p=[Diagnostics.Process]::Start($s)
 try{
  $stdout=$p.StandardOutput.ReadToEndAsync();$stderr=$p.StandardError.ReadToEndAsync()
  if(-not$p.WaitForExit(180000)){
   $snapshot=@(Get-CimInstance Win32_Process|Where-Object {$_.ProcessId-eq$p.Id-or$_.ParentProcessId-eq$p.Id-or($install-and$_.ExecutablePath-and$_.ExecutablePath.StartsWith($install,[StringComparison]::OrdinalIgnoreCase))}|Select-Object Name,ProcessId,ParentProcessId,ExecutablePath,SessionId)
   [IO.File]::WriteAllText((Join-Path $out ($name+'-timeout.json')),(@{seconds=$clock.Elapsed.TotalSeconds;root_pid=$p.Id;processes=$snapshot}|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false));throw ('Owned process timeout '+$name)
  }
  Require ($stdout.Wait(5000)-and$stderr.Wait(5000)) 'Process streams not complete';$v=@{exit=$p.ExitCode;stdout=$stdout.Result;stderr=$stderr.Result;seconds=$clock.Elapsed.TotalSeconds};[IO.File]::WriteAllText((Join-Path $out ($name+'-process.json')),($v|ConvertTo-Json),[Text.UTF8Encoding]::new($false));Require ($v.exit-eq$expectedExit) ('Native child failed '+$name);return $v
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

 function CloseOwned($handle){
  Invoke $handle 'Close window';$deadline=[DateTime]::UtcNow.AddSeconds(15)
  do{if(-not[DpiDesktop]::IsWindowVisible($handle)){return};Pump}while([DateTime]::UtcNow-lt$deadline)
  throw 'Invoked Close window did not close its native viewport'
 }
 function VisibleText($window,$name){
  $b=[DpiDesktop]::Bounds($window)
  return @(Nodes $window $name|Where-Object {$r=$_.Current.BoundingRectangle;$_.Current.ControlType-eq[Windows.Automation.ControlType]::Text-and$r.Top-gt$b[1]+75-and$r.Bottom-lt$b[1]+$b[3]-40})
 }
 function WaitText($window,$name){
  $deadline=[DateTime]::UtcNow.AddSeconds(15)
  do{if(@(VisibleText $window $name).Count-gt0){return};Pump}while([DateTime]::UtcNow-lt$deadline)
  [PickerNative]::Shot($window,(Join-Path $out ($renderer+'-missing-update.png')));throw ('Native visible text did not update without input: '+$name)
 }
 function RevealText($window,$name){
  for($scroll=0;$scroll-lt16;$scroll++){if(@(VisibleText $window $name).Count-eq1){return};[PickerNative]::Wheel($window,-480);Pump}
  throw ('Text is not visible in native viewport: '+$name)
 }
 function TypeCorners($editor,$value){
  [PickerNative]::Focus($editor);$b=[DpiDesktop]::Bounds($editor)
  for($scroll=0;$scroll-lt16;$scroll++){
   $n=Node $editor 'Window corners' ([Windows.Automation.ControlType]::Spinner);$bounds=$n.Current.BoundingRectangle
   if($bounds.Top-gt$b[1]+100-and$bounds.Bottom-lt$b[1]+$b[3]-100){break}
   [DpiDesktop]::WheelAt([int]($bounds.Left+20),[int]($b[1]+$b[3]/2),-480);Pump
  }
  Require ($bounds.Top-gt$b[1]+100-and$bounds.Bottom-lt$b[1]+$b[3]-100) 'Numeric property is outside the native viewport'
  $n.SetFocus();$deadline=[DateTime]::UtcNow.AddSeconds(5)
  do{$n=Node $editor 'Window corners' ([Windows.Automation.ControlType]::Spinner);if($n.Current.HasKeyboardFocus){break};Pump}while([DateTime]::UtcNow-lt$deadline)
  Require $n.Current.HasKeyboardFocus 'Numeric control did not acknowledge focus'
  [Windows.Forms.SendKeys]::SendWait('^a');[Windows.Forms.SendKeys]::SendWait([string]$value);[Windows.Forms.SendKeys]::SendWait('{ENTER}')
  $deadline=[DateTime]::UtcNow.AddSeconds(5)
  do{$actual=(Node $editor 'Window corners' ([Windows.Automation.ControlType]::Spinner)).GetCurrentPattern([Windows.Automation.RangeValuePattern]::Pattern).Current.Value;if($actual-eq$value){return};Pump}while([DateTime]::UtcNow-lt$deadline)
  throw 'Typed numeric value did not commit'
 }
 Phase 'malformed settings reject CLI and daemon without changing bytes'
 [IO.File]::WriteAllText($pref,'{ invalid owned settings',[Text.UTF8Encoding]::new($false));$invalidHash=(Get-FileHash $pref).Hash
 foreach($probe in @(@{exe=$app;args='index status --json';name='invalid-cli'},@{exe=(Join-Path $install 'bin\ottrin-indexd.exe');args='';name='invalid-daemon'})){
  $failure=Run $probe.exe $probe.args $probe.name 1
  Require ($failure.stderr.Contains($pref)-and$failure.stderr.Contains('preserved')) 'Failure did not identify preserved settings'
  Require ((Get-FileHash $pref).Hash-eq$invalidHash) 'Invalid settings changed'
  $report.cases+=@{case=$probe.name;exit=$failure.exit;original_bytes_preserved=$true};Save
 }
 Phase 'native startup error is visible and dismissible'
 $info=[Diagnostics.ProcessStartInfo]::new($app);$info.UseShellExecute=$false;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true;$p=[Diagnostics.Process]::Start($info);$owned.Add($p);$stdout=$p.StandardOutput.ReadToEndAsync();$stderr=$p.StandardError.ReadToEndAsync()
 $errorWindow=Await $p.Id 'Ottrin startup error';[PickerNative]::Place($errorWindow);[PickerNative]::Focus($errorWindow)
 $texts=@((Tree $errorWindow).FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)|Where-Object {$_.Current.Name.Contains('Could not load Ottrin settings')-and$_.Current.Name.Contains('preserved')})
 Require ($texts.Count-ge1) 'Native startup dialog did not explain preserved settings';$visibleError=$texts[0].Current.Name;[PickerNative]::Shot($errorWindow,(Join-Path $out 'invalid-settings-native.png'))
 $report.error_dialog_controls=@((Tree $errorWindow).FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)|ForEach-Object {@{name=$_.Current.Name;type=$_.Current.ControlType.ProgrammaticName;offscreen=$_.Current.IsOffscreen}});Save
 # This is a standard Win32 message box. Use its focused default button's
 # native Enter action rather than the egui-specific exact-name helper.
 Key $errorWindow '{ENTER}';Require ($p.WaitForExit(15000)-and$p.ExitCode-eq1) 'Dismissed startup failure did not exit with failure';Require ($stdout.Wait(5000)-and$stderr.Wait(5000)) 'Startup error streams did not close';Require ((Get-FileHash $pref).Hash-eq$invalidHash) 'Startup changed invalid settings'
 $report.cases+=@{case='native invalid-settings startup dialog';visible_error=$visibleError;exit=1;original_bytes_preserved=$true};Save
 foreach($renderer in @('Native','OpenGL')){
  Phase ($renderer+' initialize valid settings and native editor')
  $config.theme='Dark';$config.window_size=@(1000,700);$config.graphics_renderer=if($renderer-eq'Native'){'Native'}else{'OpenGlCompatibility'};$config.active_custom_theme='Settings audit';$config.custom_themes=@(@{name='Settings audit';base_preset='Ottrin';dark=@{app_radius=0};light=@{app_radius=0}});$rendererArgs=if($renderer-eq'OpenGL'){' --renderer opengl'}else{''};[IO.File]::WriteAllText($pref,($config|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false))
  $p=[Diagnostics.Process]::Start([Diagnostics.ProcessStartInfo]::new($app,('"'+$corpus+'"'+$rendererArgs)));$owned.Add($p);$w=Await $p.Id 'Ottrin';$null=Node $w 'Menu' ([Windows.Automation.ControlType]::Button)
  Invoke $w 'Menu';Invoke $w ('Settings'+[char]0x2026);$settings=Await $p.Id 'Ottrin Settings';[PickerNative]::Focus($settings);(Node $settings 'Appearance' ([Windows.Automation.ControlType]::RadioButton)).GetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern).Select();Pump
  $edit=Node $settings 'Edit Settings audit' ([Windows.Automation.ControlType]::Button);$b=[DpiDesktop]::Bounds($settings)
  for($scroll=0;$scroll-lt16;$scroll++){$bounds=$edit.Current.BoundingRectangle;if($bounds.Top-gt$b[1]+75-and$bounds.Bottom-lt$b[1]+$b[3]-75){break};[PickerNative]::Wheel($settings,-480);Pump;$edit=Node $settings 'Edit Settings audit' ([Windows.Automation.ControlType]::Button)}
  Require ($bounds.Top-gt$b[1]+75-and$bounds.Bottom-lt$b[1]+$b[3]-75) 'Theme edit action not visibly inside settings';$edit.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke();$editor=Await $p.Id 'Ottrin Theme Editor';[PickerNative]::Place($editor);TypeCorners $editor 24
  Phase ($renderer+' actual theme save refusal retains the draft')
  $before=(Get-FileHash $pref).Hash;$originalAttributes=[IO.File]::GetAttributes($pref);$readonlyConfig=$pref;[IO.File]::SetAttributes($pref,($originalAttributes-bor[IO.FileAttributes]::ReadOnly));Invoke $editor 'Save changes'
  $deadline=[DateTime]::UtcNow.AddSeconds(15)
  do{$errors=@((Tree $editor).FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)|Where-Object {$_.Current.Name.Contains('Could not save Ottrin settings')-and$_.Current.Name.Contains('read-only')});if($errors.Count){break};Pump}while([DateTime]::UtcNow-lt$deadline)
  Require ($errors.Count-ge1-and[DpiDesktop]::IsWindowVisible($editor)) 'Save error/draft not retained in native editor';Require ((Get-FileHash $pref).Hash-eq$before) 'Failed save changed original preferences';Require ((Node $editor 'Window corners' ([Windows.Automation.ControlType]::Spinner)).GetCurrentPattern([Windows.Automation.RangeValuePattern]::Pattern).Current.Value-eq24) 'Draft numeric value was lost'
  [PickerNative]::Shot($editor,(Join-Path $out ($renderer+'-save-refused.png')));$report.cases+=@{renderer=$renderer;case='native failed theme save retains draft';radius=24;original_bytes_preserved=$true;error=$errors[0].Current.Name};Save
  [IO.File]::SetAttributes($pref,$originalAttributes);$readonlyConfig=$null;Invoke $editor 'Save changes';$deadline=[DateTime]::UtcNow.AddSeconds(15)
  while([DpiDesktop]::IsWindowVisible($editor)){Require ([DateTime]::UtcNow-lt$deadline) 'Successful retry did not close editor';Pump}
  $saved=Get-Content $pref -Raw|ConvertFrom-Json;Require ($saved.custom_themes[0].dark.app_radius-eq24-and$saved.custom_themes[0].light.app_radius-eq0) 'Retry did not persist retained draft';[PickerNative]::Focus($settings);[PickerNative]::Shot($settings,(Join-Path $out ($renderer+'-save-retried.png')));$report.cases+=@{renderer=$renderer;case='native repaired save retry persisted';dark_radius=24;light_radius=0};Save
  Phase ($renderer+' actual Search backend and login status')
  (Node $settings 'Search' ([Windows.Automation.ControlType]::RadioButton)).GetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern).Select();Pump;RevealText $settings 'Start index at login';$null=Node $settings 'active in the index daemon' ([Windows.Automation.ControlType]::Text);Require (@(Nodes $settings 'active while Ottrin is running').Count-eq0) 'Stale watcher lifetime wording remains'
  [PickerNative]::Shot($settings,(Join-Path $out ($renderer+'-search-health.png')));$report.cases+=@{renderer=$renderer;case='native Search separates daemon runtime and login';dpi=[DpiDesktop]::GetDpiForWindow($settings)};Save
  # Services has its own independently required native actor and receipt.
  CloseOwned $settings;[PickerNative]::Focus($w);Invoke $w 'Close window';Require ($p.WaitForExit(15000)-and$p.ExitCode-eq0) 'Normal close failed';$saved=Get-Content $pref -Raw|ConvertFrom-Json;Require ($saved.default_view_mode-eq'List'-and$saved.custom_themes[0].dark.app_radius-eq24) 'Saved preferences changed on close';Require ((Get-FileHash $source).Hash-eq$sourceHash) 'Source changed'
 }
 $saved=(Get-FileHash $pref).Hash;Phase 'supported uninstall';$uninstaller=Join-Path $out 'owned-uninstall.exe';Copy-Item (Join-Path $install 'Uninstall.exe') $uninstaller;$null=Run $uninstaller ('/S _?='+$install) 'uninstall';Remove-Item $uninstaller;Require (-not(Test-Path $install)) 'Installation remains';Require ((Get-FileHash $pref).Hash-eq$saved) 'Uninstall changed preferences';Require ((Get-FileHash $source).Hash-eq$sourceHash) 'Source changed';$report.preferences_preserved=$true;$report.source_preserved=$true;$final='pass native settings rejection/save-retry and Search status workflow'
}catch{
 $report.error=$_.Exception.Message;$report.stack=$_.ScriptStackTrace
 if($settings-and[DpiDesktop]::IsWindowVisible($settings)){
  try{[PickerNative]::Shot($settings,(Join-Path $out 'failure-settings.png'));$report.failure_controls=@((Tree $settings).FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)|ForEach-Object {@{name=$_.Current.Name;type=$_.Current.ControlType.ProgrammaticName;enabled=$_.Current.IsEnabled;offscreen=$_.Current.IsOffscreen}})}catch{$report.failure_capture_error=$_.Exception.Message}
 }
 Save
}
finally{
 if($readonlyConfig){try{[IO.File]::SetAttributes($readonlyConfig,$originalAttributes)}catch{$report.permissions_cleanup_error=$_.Exception.Message;$final='failed owned config permissions cleanup'}}
 foreach($p in $owned){if(-not$p.HasExited){& "$env:SystemRoot\System32\taskkill.exe" /PID $p.Id /T /F|Out-Null;$null=$p.WaitForExit(10000)}}
 if($install-and(Test-Path $app)-and$manifest-and(Get-FileHash $app).Hash.ToLowerInvariant()-eq$manifest.'bin/ottrin.exe'){try{$report.cleanup_index_remove=Run $app 'index service-remove' 'cleanup-index-remove'}catch{$report.cleanup_index_error=$_.Exception.Message;$final='failed owned index teardown'}}
 if($install){$left=@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath-and$_.ExecutablePath.StartsWith($install,[StringComparison]::OrdinalIgnoreCase)});$report.remaining_owned_installed_processes=$left.Count;if($left.Count){$final='failed owned process teardown'}}
 $report.status=$final;Save
}
if(-not$final.StartsWith('pass')){exit 1}
