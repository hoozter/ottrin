param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExpectedSid)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$out=Join-Path $Root 'results';$report=@{status='running';scope='Exact current NSIS, same signed VirtIO144DPI/private one-file root as748a baseline; static main GUI CPU, background index status and new OS storage root reach the native UI without injected input; normal close and supported uninstall. Physical graphics, full appearance matrix and MSIX lifecycle separate';cases=@();gui_idle_baseline=@()};$final='failed native DPI/layout audit';$owned=[Collections.Generic.List[Diagnostics.Process]]::new();$install=$null
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
using System;using System.Text;using System.Runtime.InteropServices;
public static class CpuProbe {
 [DllImport("user32.dll")]public static extern uint GetGuiResources(IntPtr process,uint flags);
 [DllImport("kernel32.dll")]public static extern uint GetLogicalDrives();
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)]static extern uint QueryDosDeviceW(string device,StringBuilder target,uint size);
 public static string DriveTarget(){var text=new StringBuilder(32768);if(QueryDosDeviceW("Q:",text,32768)==0)throw new Exception("Cannot inspect owned drive mapping: "+Marshal.GetLastWin32Error());return text.ToString();}
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



 function MeasureGuiIdle($process,$window){
  Phase ($renderer+' separate GUI idle baseline');$deadline=[DateTime]::UtcNow.AddSeconds(60);$attempt=0
  do{$attempt++;$state=Run $app 'index status --json' ('idle-index-'+$renderer+'-'+$attempt);$index=$state.stdout|ConvertFrom-Json;if($index.status-eq'ready'-and$index.watcher_ready){break};Start-Sleep -Milliseconds 500}while([DateTime]::UtcNow-lt$deadline)
  if($index.status-ne'ready'-or-not$index.watcher_ready){$report.gui_idle_baseline+=@{renderer=$renderer;status='unmeasured: index not ready';index=$index};Save;return}
  [PickerNative]::Place($window);[PickerNative]::Focus($window);$area=[DpiDesktop]::Work($window);[Windows.Forms.Cursor]::Position=[Drawing.Point]::new($area[2]-2,$area[3]-2);Start-Sleep -Seconds 5
  $process.Refresh();$before=$process.TotalProcessorTime.TotalSeconds;$rssBefore=$process.WorkingSet64;$gdiBefore=[CpuProbe]::GetGuiResources($process.Handle,0);$clock=[Diagnostics.Stopwatch]::StartNew();Start-Sleep -Seconds 30;$process.Refresh();$seconds=$clock.Elapsed.TotalSeconds;$cpu=100*($process.TotalProcessorTime.TotalSeconds-$before)/$seconds;$rssAfter=$process.WorkingSet64;$gdiAfter=[CpuProbe]::GetGuiResources($process.Handle,0)
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
 function AliasRows($window){
  $condition=[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,[Windows.Automation.ControlType]::Button)
  return @((Tree $window).FindAll([Windows.Automation.TreeScope]::Descendants,$condition)|Where-Object {-not$_.Current.IsOffscreen-and($_.Current.Name.Contains($corpus)-or$_.Current.Name.Contains('Q:'))-and$_.Current.Name.Contains('Indexed search: disabled')})
 }
 foreach($renderer in @('Native','OpenGL')){
  Phase ($renderer+' initialize exact baseline configuration')
  $config.theme='Dark';$config.window_size=@(1000,700);$config.graphics_renderer=if($renderer-eq'Native'){'Native'}else{'OpenGlCompatibility'};$config.active_custom_theme='Radius audit';$config.custom_themes=@(@{name='Radius audit';base_preset='Ottrin';dark=@{app_radius=0};light=@{app_radius=0}});$rendererArgs=if($renderer-eq'OpenGL'){' --renderer opengl'}else{''};[IO.File]::WriteAllText($pref,($config|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false))
  $s=[Diagnostics.ProcessStartInfo]::new($app,('"'+$corpus+'"'+$rendererArgs));$s.UseShellExecute=$false;$p=[Diagnostics.Process]::Start($s);$owned.Add($p);$w=Await $p.Id 'Ottrin';$null=Node $w 'Menu' ([Windows.Automation.ControlType]::Button)
  MeasureGuiIdle $p $w
  $sample=$report.gui_idle_baseline[-1];Require ($sample.index_before.indexed_items-eq1-and$sample.index_after.indexed_items-eq1) 'Baseline corpus or index state differs';Require ([DpiDesktop]::Fits($w)-and[DpiDesktop]::GetDpiForWindow($w)-eq144) 'Baseline viewport differs'
  [PickerNative]::Shot($w,(Join-Path $out ($renderer+'-idle-main.png')))
  Phase ($renderer+' index updates while settings receives no input')
  Invoke $w 'Menu';Invoke $w ('Settings'+[char]0x2026);$settings=Await $p.Id 'Ottrin Settings';[PickerNative]::Focus($settings)
  (Node $settings 'Search' ([Windows.Automation.ControlType]::RadioButton)).GetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern).Select();Pump
  $found=$false;for($scroll=0;$scroll-lt16;$scroll++){if(@(VisibleText $settings '1 files indexed').Count-gt0){$found=$true;break};[PickerNative]::Wheel($settings,-480);Pump};Require $found 'Initial native index status is not visible'
  $area=[DpiDesktop]::Work($settings);[Windows.Forms.Cursor]::Position=[Drawing.Point]::new($area[2]-2,$area[3]-2)
  $needle=Join-Path $corpus 'observed-needle.txt';Require (-not(Test-Path $needle)) 'Existing mutation file retained';[IO.File]::WriteAllText($needle,'owned external change',[Text.UTF8Encoding]::new($false))
  WaitText $settings '2 files indexed';[PickerNative]::Shot($settings,(Join-Path $out ($renderer+'-index-updated.png')))
  Remove-Item $needle;WaitText $settings '1 files indexed'
  $report.cases+=@{renderer=$renderer;case='native index label follows external create/delete without input';counts=@(1,2,1);dpi=[DpiDesktop]::GetDpiForWindow($settings)};Save
  CloseOwned $settings;[PickerNative]::Focus($w)
  Phase ($renderer+' new OS drive root reaches sleeping locations rail')
  Require (([CpuProbe]::GetLogicalDrives()-band(1-shl16))-eq0) 'Existing Q drive retained';Require (@(AliasRows $w).Count-eq0) 'Existing corpus alias retained'
  $driveCleanupNeeded=$true;$null=Run "$env:SystemRoot\System32\subst.exe" ('Q: "'+$corpus+'"') ('drive-create-'+$renderer)
  $driveTarget=[CpuProbe]::DriveTarget();Require ($driveTarget.TrimEnd([char]92)-eq('\??\'+$corpus).TrimEnd([char]92)) 'Owned drive mapping differs';Require (([CpuProbe]::GetLogicalDrives()-band(1-shl16))-ne0) 'Native OS drive mask did not change'
  $deadline=[DateTime]::UtcNow.AddSeconds(15)
  do{$aliases=@(AliasRows $w);if($aliases.Count-eq1){break};Pump}while([DateTime]::UtcNow-lt$deadline)
  Require ($aliases.Count-eq1) 'Native locations rail did not observe the new OS root without input'
  [PickerNative]::Shot($w,(Join-Path $out ($renderer+'-storage-updated.png')))
  $report.cases+=@{renderer=$renderer;case='new DOS device root visible without injected input';label=$aliases[0].Current.Name;scope='Insertion through actual GetLogicalDrives/canonical path discovery; subst is not physical removable-volume removal/reconnect';dpi=[DpiDesktop]::GetDpiForWindow($w)};Save
  Require ([CpuProbe]::DriveTarget()-eq$driveTarget) 'Drive changed before owned cleanup';$null=Run "$env:SystemRoot\System32\subst.exe" 'Q: /D' ('drive-remove-'+$renderer);$driveCleanupNeeded=$false
  Invoke $w 'Close window';Require ($p.WaitForExit(15000)-and$p.ExitCode-eq0) 'Normal main close failed'
  $savedConfig=Get-Content $pref -Raw|ConvertFrom-Json;Require ($savedConfig.default_view_mode-eq'List'-and$savedConfig.search.include_roots.Count-eq1-and$savedConfig.search.include_roots[0]-eq$corpus-and$savedConfig.search.indexed_volumes.Count-eq0-and-not$savedConfig.search.content_indexing_enabled) 'Configured preferences or storage opt-in changed';Require ((Get-FileHash $source).Hash-eq$sourceHash) 'Source changed'
 }
 $saved=(Get-FileHash $pref).Hash;Phase 'supported uninstall';$uninstaller=Join-Path $out 'owned-uninstall.exe';Copy-Item (Join-Path $install 'Uninstall.exe') $uninstaller;$null=Run $uninstaller ('/S _?='+$install) 'uninstall';Remove-Item $uninstaller;Require (-not(Test-Path $install)) 'Installation remains';Require ((Get-FileHash $pref).Hash-eq$saved) 'Uninstall changed preferences';Require ((Get-FileHash $source).Hash-eq$sourceHash) 'Source changed';$report.preferences_preserved=$true;$report.source_preserved=$true;$final='pass native idle observation workflow; CPU values require separate criterion assessment'
}catch{$report.error=$_.Exception.Message;$report.stack=$_.ScriptStackTrace}
finally{
 if($driveCleanupNeeded-and(([CpuProbe]::GetLogicalDrives()-band(1-shl16))-ne0)){try{Require ([CpuProbe]::DriveTarget()-eq('\??\'+$corpus)) 'Foreign drive mapping retained';$null=Run "$env:SystemRoot\System32\subst.exe" 'Q: /D' 'cleanup-drive';$driveCleanupNeeded=$false}catch{$report.drive_cleanup_error=$_.Exception.Message;$final='failed owned drive cleanup'}}
 foreach($p in $owned){if(-not$p.HasExited){& "$env:SystemRoot\System32\taskkill.exe" /PID $p.Id /T /F|Out-Null;$null=$p.WaitForExit(10000)}}
 if($install-and(Test-Path $app)-and$manifest-and(Get-FileHash $app).Hash.ToLowerInvariant()-eq$manifest.'bin/ottrin.exe'){try{$report.cleanup_index_remove=Run $app 'index service-remove' 'cleanup-index-remove'}catch{$report.cleanup_index_error=$_.Exception.Message;$final='failed owned index teardown'}}
 if($install){$left=@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath-and$_.ExecutablePath.StartsWith($install,[StringComparison]::OrdinalIgnoreCase)});$report.remaining_owned_installed_processes=$left.Count;if($left.Count){$final='failed owned process teardown'}}
 $report.status=$final;Save
}
if(-not$final.StartsWith('pass')){exit 1}
