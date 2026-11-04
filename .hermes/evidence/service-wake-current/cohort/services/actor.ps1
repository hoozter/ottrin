param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExpectedSid)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$out=Join-Path $Root 'results';$report=@{status='running';scope='Exact current NSIS event-driven Services workflow and independent login checkbox matrix; decoded child diagnostics are read-only. No broader Linux/MSIX/login/physical acceptance.';cases=@()};$final='failed independent native Services workflow';$readonlyConfig=$null;$originalAttributes=$null;$owned=[Collections.Generic.List[Diagnostics.Process]]::new();$install=$null
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
 # Actual per-user service and independent login controls through native UI.
 # Reboot/login activation, MSIX and Linux polkit remain separate scopes.
 function OwnedIndexTask {
  $scheduler=New-Object -ComObject Schedule.Service;$scheduler.Connect()
  try{$task=$scheduler.GetFolder('\').GetTask(('Ottrin Index '+$ExpectedSid))}
  catch{if($_.Exception.GetBaseException().HResult-eq-2147024894){return $null};throw}
  $principal=$task.Definition.Principal.UserId
  if(-not$principal.StartsWith('S-1-')){$principal=([Security.Principal.NTAccount]::new($principal)).Translate([Security.Principal.SecurityIdentifier]).Value}
  Require ($principal-eq$ExpectedSid-and$task.Definition.Principal.LogonType-eq3-and$task.Definition.Principal.RunLevel-eq0) 'Task owner/logon privilege differs'
  Require ($task.Definition.Actions.Item(1).Path-eq(Join-Path $install 'bin\ottrin-indexd.exe')-and$task.Definition.Actions.Item(1).Arguments-eq'--index-daemon') 'Task does not target the current daemon'
  return $task
 }
 # The original60second in-flight failure is retained. This longer diagnostic
 # observation measures terminal behavior on the cold software VM; it does not
 # accept60second responsiveness or change production deadlines.
 function ServicePending($window,$expected,$clock){
  if($clock.Elapsed.TotalSeconds-lt10){return}
  if($script:next_service_probe-and[DateTime]::UtcNow-lt$script:next_service_probe){return}
  $script:next_service_probe=[DateTime]::UtcNow.AddSeconds(10)
  $children=@(Get-CimInstance Win32_Process -Filter ('ParentProcessId='+$p.Id)|Where-Object {$_.Name-eq'powershell.exe'})
  foreach($child in $children){
   if(@($report.service_pending|Where-Object {$_.pid-eq$child.ProcessId}).Count){continue}
   $decoded=$null
   if($child.CommandLine-match'-EncodedCommand\s+"?([A-Za-z0-9+/=]+)'){$decoded=[Text.Encoding]::Unicode.GetString([Convert]::FromBase64String($Matches[1]))}
   $stage=if(-not$decoded){'not decoded'}elseif($decoded.Contains('$definition = $service.NewTask')){'register'}elseif($decoded.Contains('[Console]::Out.Write($task.Xml)')){'query XML and owner'}elseif($decoded.Contains('[Console]::Out.Write((Get-OttrinIndexTaskName))')){'resolve task name'}elseif($decoded.Contains('OTTRIN_TASK_ENABLED')){'change login flag'}else{'stop task or other fixed scheduler script'}
   $report.service_pending+=@{expected=$expected;seconds=$clock.Elapsed.TotalSeconds;pid=$child.ProcessId;parent=$child.ParentProcessId;executable=$child.ExecutablePath;created=[string]$child.CreationDate;stage=$stage;fixed_script=$decoded};Save
  }
 }
 function ServiceButton($window,$name){
  $deadline=[DateTime]::UtcNow.AddSeconds(180);$clock=[Diagnostics.Stopwatch]::StartNew()
  do{
   $nodes=@(Nodes $window $name|Where-Object {$_.Current.ControlType-eq[Windows.Automation.ControlType]::Button-and$_.Current.IsEnabled})
   if($name-eq'Refresh status'){
    $allCount=$nodes.Count;$start=@(VisibleText $window 'Background index service');$end=@(VisibleText $window 'Desktop integration')
    if($start.Count-eq1-and$end.Count-eq1){
     $top=$start[0].Current.BoundingRectangle.Bottom;$bottom=$end[0].Current.BoundingRectangle.Top
     $nodes=@($nodes|Where-Object {$_.Current.BoundingRectangle.Top-ge$top-and$_.Current.BoundingRectangle.Bottom-le$bottom})
    }else{$nodes=@()}
    if($nodes.Count-eq1){$report.index_refresh_selector=@{all_named_buttons=$allCount;index_section_buttons=$nodes.Count;section_start='Background index service';section_end='Desktop integration';target_bounds=$nodes[0].Current.BoundingRectangle.ToString()};Save}
   }
   if($nodes.Count-eq1){return $nodes[0]};ServicePending $window $name $clock;Pump
  }while([DateTime]::UtcNow-lt$deadline)
  throw ('Service action is not enabled: '+$name)
 }
 function ServiceText($window,$name){
  $deadline=[DateTime]::UtcNow.AddSeconds(180);$clock=[Diagnostics.Stopwatch]::StartNew()
  do{if(@(VisibleText $window $name).Count-eq1){return};ServicePending $window $name $clock;Pump}while([DateTime]::UtcNow-lt$deadline)
  [PickerNative]::Shot($window,(Join-Path $out 'service-state-missing.png'));throw ('Native service state not visible: '+$name)
 }
 function BeginServiceTiming($name){$p.Refresh();$script:service_clock=[Diagnostics.Stopwatch]::StartNew();$script:service_cpu=$p.TotalProcessorTime.TotalSeconds;$script:service_name=$name}
 function InvokeService($window,$name){$button=ServiceButton $window $name;BeginServiceTiming $name;$button.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke();Pump}
 function ObserveService($window,$case,$installed,$enabled,$running,$scheduled=$null){
  ServiceText $window $(if($running){'Running '+[char]0x2014+' index ready'}else{'Stopped'})
  $setup=if($installed){'Update / reinstall service'}else{'Enable automatic startup'};$null=ServiceButton $window $setup
  Require (@(Nodes $window $(if($installed){'Enable automatic startup'}else{'Update / reinstall service'})).Count-eq0) 'Setup action does not match registration'
  Require ((@(Nodes $window 'Uninstall service').Count-eq1)-eq$installed) 'Uninstall action does not match registration'
  Require ((@(Nodes $window 'Start index automatically at sign-in').Count-eq1)-eq$installed) 'Login checkbox does not match registration'
  $expectedLogin=if(-not$installed){'Automatic startup at sign-in: Off'}elseif($enabled){'Automatic startup at sign-in: Enabled at login'}else{'Automatic startup at sign-in: Installed but disabled'};ServiceText $window $expectedLogin
  $deadline=[DateTime]::UtcNow.AddSeconds(30)
  do{$task=OwnedIndexTask;if(($null-ne$task)-eq$installed-and(-not$installed-or$task.Enabled-eq$enabled)){break};Pump}while([DateTime]::UtcNow-lt$deadline)
  Require (($null-ne$task)-eq$installed-and(-not$installed-or$task.Enabled-eq$enabled)) 'Actual task registration differs from UI'
  $deadline=[DateTime]::UtcNow.AddSeconds(30)
  do{$daemonCount=@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue).Count;$instances=if($task){$task.GetInstances(0).Count}else{0};if((($daemonCount-gt0)-eq$running)-and($running-or$instances-eq0)-and($null-eq$scheduled-or(($instances-gt0)-eq$scheduled))){break};Pump}while([DateTime]::UtcNow-lt$deadline)
  Require ((($daemonCount-gt0)-eq$running)-and($running-or$instances-eq0)-and($null-eq$scheduled-or(($instances-gt0)-eq$scheduled))) 'Actual daemon/task runtime state differs from UI'
  if($script:service_clock){$p.Refresh();$report.service_timings+=@{action=$script:service_name;case=$case;seconds=$script:service_clock.Elapsed.TotalSeconds;app_cpu_seconds=$p.TotalProcessorTime.TotalSeconds-$script:service_cpu};$script:service_clock=$null}
  $report.service_matrix.states+=@{case=$case;installed=$installed;login_enabled=$enabled;running=$running;native_setup_action=$setup;native_login=$expectedLogin;daemon_processes=$daemonCount;task_name=if($task){$task.Name}else{$null};task_enabled=if($task){$task.Enabled}else{$null};task_instances=if($task){$task.GetInstances(0).Count}else{0}};Save
 }
 function ToggleLogin($window,$case,$enabled,$running){
  $before=@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue|ForEach-Object {$_.Id}|Sort-Object)
  $task=OwnedIndexTask;$instances=$task.GetInstances(0).Count;$security=$task.GetSecurityDescriptor(7)
  $choice=Node $window 'Start index automatically at sign-in' ([Windows.Automation.ControlType]::CheckBox)
  BeginServiceTiming $case
  $choice.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern).Toggle();Pump
  ObserveService $window $case $true $enabled $running
  $after=@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue|ForEach-Object {$_.Id}|Sort-Object)
  Require (($before-join',')-eq($after-join',')) 'Login choice started/stopped/replaced the current daemon'
  $task=OwnedIndexTask;Require ($task.GetInstances(0).Count-eq$instances-and$task.GetSecurityDescriptor(7)-eq$security) 'Login choice changed task runtime or permissions'
  $state=(Node $window 'Start index automatically at sign-in' ([Windows.Automation.ControlType]::CheckBox)).GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern).Current.ToggleState
  Require (($state-eq[Windows.Automation.ToggleState]::On)-eq$enabled) 'Native login checkbox does not match actual task flag'
  $report.service_matrix.login_toggle_checks+=@{case=$case;enabled=$enabled;running=$running;daemon_pids_preserved=$after;task_instances_preserved=$instances;permissions_preserved=$true};Save
 }
 function RunServicesAudit($settings){
  Phase 'Native service registration/runtime and independent login checkbox matrix'
  $report.service_pending=@();$report.service_timings=@();$report.service_matrix=@{status='running';states=@();login_toggle_checks=@();scope='Ordinary-user current NSIS, existing Task Scheduler service controls. No reboot/login, MSIX startup, Linux polkit or full U04 acceptance.'};Save
  [PickerNative]::Focus($settings);(Node $settings 'Services' ([Windows.Automation.ControlType]::RadioButton)).GetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern).Select();Pump
  [PickerNative]::Wheel($settings,2400);Pump;[PickerNative]::Wheel($settings,2400);Pump
  ObserveService $settings 'session running; login not installed' $false $false $true
  [PickerNative]::Shot($settings,(Join-Path $out 'services-session-running.png'))
  $preferencesBefore=(Get-FileHash $pref).Hash
  InvokeService $settings 'Enable automatic startup';ObserveService $settings 'installed enabled running' $true $true $true $true
  $task=OwnedIndexTask;$security=$task.GetSecurityDescriptor(7)
  [PickerNative]::Shot($settings,(Join-Path $out 'services-installed-running.png'))
  ToggleLogin $settings 'login disabled while task stays running' $false $true
  [PickerNative]::Shot($settings,(Join-Path $out 'services-disabled-running.png'))
  ToggleLogin $settings 'login reenabled while task stays running' $true $true
  InvokeService $settings 'Update / reinstall service';ObserveService $settings 'reinstalled enabled running' $true $true $true $true;Require ((OwnedIndexTask).GetSecurityDescriptor(7)-eq$security) 'Reinstall changed task permissions'
  InvokeService $settings 'Stop indexing';ObserveService $settings 'installed enabled stopped' $true $true $false
  [PickerNative]::Shot($settings,(Join-Path $out 'services-installed-stopped.png'))
  ToggleLogin $settings 'login disabled while index stays stopped' $false $false
  [PickerNative]::Shot($settings,(Join-Path $out 'services-disabled-stopped.png'))
  ToggleLogin $settings 'login reenabled while index stays stopped' $true $false
  InvokeService $settings 'Start / restart';ObserveService $settings 'installed enabled restarted' $true $true $true $true
  # Simulate the owner's supported external Windows startup choice. Repair must
  # retain it rather than silently opting back into automatic startup.
  $task=OwnedIndexTask;$task.Enabled=$false;InvokeService $settings 'Refresh status';ObserveService $settings 'installed disabled but running' $true $false $true
  InvokeService $settings 'Update / reinstall service';ObserveService $settings 'disabled registration repaired; runtime resumed' $true $false $true;Require ((OwnedIndexTask).GetSecurityDescriptor(7)-eq$security) 'Disabled repair changed task permissions'
  [PickerNative]::Shot($settings,(Join-Path $out 'services-disabled-running.png'))
  InvokeService $settings 'Stop indexing';ObserveService $settings 'installed disabled stopped' $true $false $false
  InvokeService $settings 'Start / restart';ObserveService $settings 'disabled login; session restarted' $true $false $true
  ToggleLogin $settings 'login enabled while existing session stays running' $true $true
  [PickerNative]::Shot($settings,(Join-Path $out 'services-enabled-session-running.png'))
  ToggleLogin $settings 'login disabled again while session stays running' $false $true
  InvokeService $settings 'Uninstall service';ObserveService $settings 'uninstalled and stopped' $false $false $false
  [PickerNative]::Shot($settings,(Join-Path $out 'services-uninstalled.png'))
  Require ((Get-FileHash $pref).Hash-eq$preferencesBefore) 'Service actions changed main settings'
  Require ((Get-FileHash $source).Hash-eq$sourceHash) 'Service actions changed source'
  $persisted=(Run $app 'index status --json' 'service-persisted-index').stdout|ConvertFrom-Json
  Require ($persisted.indexed_items-eq1-and-not$persisted.watcher_ready) 'Service removal did not keep the saved index or claimed a live watcher'
  $report.service_matrix.status='pass native service and independent login state scope';$report.service_matrix.source_and_preferences_preserved=$true;$report.service_matrix.saved_index_items=$persisted.indexed_items;Save
 }
 Phase 'prepare independent Services controls'
 $config.theme='Dark';$config.graphics_renderer='OpenGlCompatibility';$config.window_size=@(1000,700)
 [IO.File]::WriteAllText($pref,($config|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false))
 $p=[Diagnostics.Process]::Start([Diagnostics.ProcessStartInfo]::new($app,('"'+$corpus+'" --renderer opengl')));$owned.Add($p)
 $w=Await $p.Id 'Ottrin';Invoke $w 'Menu';Invoke $w ('Settings'+[char]0x2026);$settings=Await $p.Id 'Ottrin Settings'
 RunServicesAudit $settings
 CloseOwned $settings;[PickerNative]::Focus($w);Invoke $w 'Close window';Require ($p.WaitForExit(15000)-and$p.ExitCode-eq0) 'Normal close failed'
 $saved=(Get-FileHash $pref).Hash;Phase 'supported uninstall';$uninstaller=Join-Path $out 'owned-uninstall.exe';Copy-Item (Join-Path $install 'Uninstall.exe') $uninstaller;$null=Run $uninstaller ('/S _?='+$install) 'uninstall';Remove-Item $uninstaller;Require (-not(Test-Path $install)) 'Installation remains';Require ((Get-FileHash $pref).Hash-eq$saved) 'Uninstall changed preferences';Require ((Get-FileHash $source).Hash-eq$sourceHash) 'Source changed';$report.preferences_preserved=$true;$report.source_preserved=$true;$final='pass independent native Services workflow and preserved uninstall'
}catch{
 $report.error=$_.Exception.Message;$report.stack=$_.ScriptStackTrace
 if($settings-and[DpiDesktop]::IsWindowVisible($settings)){
  try{[PickerNative]::Shot($settings,(Join-Path $out 'failure-settings.png'));$report.failure_controls=@((Tree $settings).FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)|ForEach-Object {@{name=$_.Current.Name;type=$_.Current.ControlType.ProgrammaticName;enabled=$_.Current.IsEnabled;offscreen=$_.Current.IsOffscreen}})}catch{$report.failure_capture_error=$_.Exception.Message}
 }
 try{$task=OwnedIndexTask;$report.failure_task=if($task){@{name=$task.Name;enabled=$task.Enabled;state=$task.State;last_result=$task.LastTaskResult;instances=$task.GetInstances(0).Count}}else{'absent'}}catch{$report.failure_task_error=$_.Exception.Message}
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
