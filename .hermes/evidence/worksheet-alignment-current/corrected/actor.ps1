param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExpectedSid)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$out=Join-Path $Root 'results';$r=$out;$owned=[Collections.Generic.List[Diagnostics.Process]]::new();$report=@{status='running';scope='Exact staged NSIS native worksheet typography, main/Inspector, Native preference and OpenGL override; ordinary-user software VM';cases=@{}};$final='failed native installed Office';$bin=$null
function Require($value,$message){if(-not$value){throw $message}}
function Save{$path=Join-Path $out 'RESULT.json';$temp=$path+'.next';[IO.File]::WriteAllText($temp,($report|ConvertTo-Json -Depth 20),[Text.UTF8Encoding]::new($false));if(Test-Path $path){[IO.File]::Replace($temp,$path,[NullString]::Value)}else{[IO.File]::Move($temp,$path)}}
function Phase($name){$report.phase=$name;Save}
function Run($exe,$arguments,$name){$start=[Diagnostics.ProcessStartInfo]::new($exe,$arguments);$start.UseShellExecute=$false;$start.CreateNoWindow=$true;$start.RedirectStandardOutput=$true;$start.RedirectStandardError=$true;$child=[Diagnostics.Process]::Start($start);try{$stdout=$child.StandardOutput.ReadToEndAsync();$stderr=$child.StandardError.ReadToEndAsync();Require ($child.WaitForExit(120000)) ('Owned process timeout: '+$name);Require ($stdout.Wait(5000)-and$stderr.Wait(5000)) 'Output not complete';$value=@{exit=$child.ExitCode;stdout=$stdout.Result;stderr=$stderr.Result};[IO.File]::WriteAllText((Join-Path $out ($name+'-process.json')),($value|ConvertTo-Json -Depth 5),[Text.UTF8Encoding]::new($false));Require ($value.exit-eq0) ('Native child failed: '+$name+' '+$value.stderr);return $value}finally{if(-not$child.HasExited){$child.Kill();$null=$child.WaitForExit(10000)};$child.Dispose()}}
& ([ScriptBlock]::Create([IO.File]::ReadAllText((Join-Path $Root 'native-controls.ps1'))));[PickerNative]::Init()
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;using System.Text;using System.Drawing;using System.Drawing.Imaging;using System.Runtime.InteropServices;
public static class OfficeUI {
 private delegate bool Visitor(IntPtr h,IntPtr l);
 [DllImport("user32.dll",SetLastError=true)] private static extern IntPtr OpenInputDesktop(uint f,bool i,uint a);
 [DllImport("user32.dll")] private static extern bool CloseDesktop(IntPtr h);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] private static extern bool GetUserObjectInformation(IntPtr h,int i,StringBuilder b,uint n,out uint needed);
 public static string InputDesktop(){var h=OpenInputDesktop(0,false,1);if(h==IntPtr.Zero)return "unavailable";try{var b=new StringBuilder(256);uint n;return GetUserObjectInformation(h,2,b,512,out n)?b.ToString():"unavailable";}finally{CloseDesktop(h);}}

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
  int orange=0,cyan=0,blue=0,green=0;for(int y=0;y<image.Height;y+=2)for(int x=0;x<image.Width;x+=2){var p=image.GetPixel(x,y);if(p.R>210&&p.G>80&&p.G<160&&p.B<70)orange++;if(p.R<30&&p.G>150&&p.B>190)cyan++;if(p.R<80&&p.G<140&&p.B>130)blue++;if(p.R<130&&p.G>90&&p.B<150)green++;}return new int[]{orange,cyan,blue,green};}}
}
'@
function Focus-Owned($window,$p) {
 $attemptError=$null;try{[PickerNative]::Focus($window)}catch{$attemptError=$_.Exception.Message}
 [uint32]$foreground=0;$null=[OfficeUI]::GetWindowThreadProcessId([OfficeUI]::GetForegroundWindow(),[ref]$foreground)
 if($foreground-ne$p.Id){
  $report.focus_wait=@{window=$window.ToInt64();application_pid=$p.Id;foreground_pid=$foreground;foreground_title=[OfficeUI]::Title([OfficeUI]::GetForegroundWindow());input_desktop=[OfficeUI]::InputDesktop();attempt_error=$attemptError;scope='Await normal native console focus; no security or foreground-policy change. Actual target PID foreground remains required.'};Save
  $deadline=[DateTime]::UtcNow.AddSeconds(120)
  do{Start-Sleep -Milliseconds 200;$null=[OfficeUI]::GetWindowThreadProcessId([OfficeUI]::GetForegroundWindow(),[ref]$foreground);if($foreground-eq$p.Id){break}}while([DateTime]::UtcNow-lt$deadline)
 }
 Require ($foreground-eq$p.Id) 'Owned Ottrin window does not have keyboard focus';Require ([OfficeUI]::InputDesktop()-eq'Default') 'Actual input desktop not ready'
 if($report.focus_wait){$report.focus_wait.normal_native_focus_observed=$true;Save}
}
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
function Wait-NativeWorksheet($p,[string]$label,[string]$name) {
 $deadline=[DateTime]::UtcNow.AddSeconds(60);$moved=$false
 do {
  Start-Sleep -Milliseconds 250;$p.Refresh();Require (-not$p.HasExited) 'Native worksheet process exited before capture'
  $window=[OfficeUI]::Find($p.Id,$label)
  if($window-eq[IntPtr]::Zero){continue}
  if(-not$moved){$bounds=[Windows.Forms.Screen]::PrimaryScreen.Bounds;$null=[OfficeUI]::MoveWindow($window,20,20,[Math]::Min(1000,$bounds.Width-40),[Math]::Min(720,$bounds.Height-40),$true);Focus-Owned $window $p;$moved=$true}
  try {
   $rootElement=[Windows.Automation.AutomationElement]::FromHandle($window);$nodes=$rootElement.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition);Require ($nodes.Count-lt5000) 'Native worksheet accessibility tree exceeded bounded observer';$names=@(foreach($node in $nodes){$node.Current.Name})
  }catch [Windows.Automation.ElementNotAvailableException]{continue}
  if($names-contains'OTTRIN SHEET FIDELITY'-and$names-contains'Currency totals and calculated formulas'){
   Start-Sleep -Milliseconds 800;$pixels=[OfficeUI]::Capture($window,(Join-Path $out ($name+'.png')))
   return @{window=$window;title=[OfficeUI]::Title($window);expected_native_cell_names=@('OTTRIN SHEET FIDELITY','Currency totals and calculated formulas');scope='Native cell names establish readiness; exact capture still requires direct glyph/row review'}
  }
 }while([DateTime]::UtcNow-lt$deadline)
 $report.last_native_names=$names;throw 'Native worksheet cell names did not become ready'
}

function Close-Owned($process,$window){Require ([OfficeUI]::PostMessage($window,0x10,[IntPtr]::Zero,[IntPtr]::Zero)) 'Normal Close refused';Require ($process.WaitForExit(15000)) 'Normal Close timed out';$process.WaitForExit();Require ($process.ExitCode-eq0) 'Normal Close failed'}
function Start-Owned($arguments,$name){$p=Start-Process $app -ArgumentList $arguments -WorkingDirectory $bin -PassThru -RedirectStandardOutput (Join-Path $out ($name+'.stdout')) -RedirectStandardError (Join-Path $out ($name+'.stderr'));$null=$p.Handle;$owned.Add($p);return $p}
function Modules($process,$renderer){$process.Refresh();$modules=@($process.Modules|Where-Object {$_.ModuleName-match'^(d3d12|d3d12core|d3d10warp|dxgi|opengl32|libgallium_wgl|vulkan-1)\.dll$'}|ForEach-Object {@{name=$_.ModuleName;path=$_.FileName;sha256=(Get-FileHash $_.FileName).Hash.ToLowerInvariant()}});if($renderer-eq'native'){Require (@($modules|Where-Object {$_.name-ieq'd3d12.dll'}).Count-eq1) 'Native D3D12 module not observed'}else{Require (@($modules|Where-Object {$_.path-ieq(Join-Path $bin 'opengl32.dll')-and$_.sha256-eq$manifest.'bin/opengl32.dll'}).Count-eq1) 'Bundled OpenGL module not observed'};return $modules}
try{
 $id=[Security.Principal.WindowsIdentity]::GetCurrent();Require ($id.User.Value-eq$ExpectedSid) 'Wrong owned SID';Require (-not([Security.Principal.WindowsPrincipal]::new($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Ordinary token required';Require ([Diagnostics.Process]::GetCurrentProcess().SessionId-gt0) 'Interactive desktop required';$report.identity=@{sid=$ExpectedSid;session=[Diagnostics.Process]::GetCurrentProcess().SessionId;administrator=$false;profile=[Environment]::GetFolderPath('UserProfile')}
 Phase 'waiting for observed owned desktop';$deadline=[DateTime]::UtcNow.AddMinutes(8);while(-not(Test-Path (Join-Path $out 'DESKTOP_READY.json'))){Require ([DateTime]::UtcNow-lt$deadline) 'Owned desktop readiness timeout';Start-Sleep -Milliseconds 200};$owner=Get-Content 'C:\OttrinMsiCleanClientResults\OWNER.json' -Raw|ConvertFrom-Json;$ready=Get-Content (Join-Path $out 'DESKTOP_READY.json') -Raw|ConvertFrom-Json;Require ($ready.sid-eq$ExpectedSid-and$ready.nonce-eq$owner.nonce) 'Wrong native desktop observation';$report.desktop_ready=$true
 $packageInput=Get-Content (Join-Path $Root 'application-input.json') -Raw|ConvertFrom-Json;foreach($f in $packageInput.files){Require ((Get-FileHash (Join-Path $Root $f.name)).Hash.ToLowerInvariant()-eq$f.sha256) 'Package input changed'};$report.package=$packageInput
 $manifest=Get-Content (Join-Path $Root 'payload.json') -Raw|ConvertFrom-Json;$install=Join-Path $env:LOCALAPPDATA 'Programs\Ottrin';$bin=Join-Path $install 'bin';$app=Join-Path $bin 'ottrin.exe';Require (-not(Test-Path $install)) 'Existing owned-account install retained'
 $provider=Join-Path $env:ProgramFiles 'LibreOffice\program';Require (Test-Path (Join-Path $provider 'mergedlo.dll')) 'Verified retained LibreOffice provider unavailable';$report.provider=@{version=(Get-Item (Join-Path $provider 'soffice.exe')).VersionInfo.FileVersion;mergedlo_sha256=(Get-FileHash (Join-Path $provider 'mergedlo.dll')).Hash;vcl_windows_sha256=(Get-FileHash (Join-Path $provider 'vclplug_winlo.dll')).Hash};Require ($report.provider.mergedlo_sha256-eq'017D0C21B6E45E54C0194B58017853D8C653059CC643643C3EB831A4D309331F') 'Retained provider changed from fidelity receipt'
 $cases=Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'OttrinOwnedOffice';Require (-not(Test-Path $cases)) 'Existing source fixture retained';$null=New-Item -ItemType Directory $cases
 foreach($f in $packageInput.fixtures.PSObject.Properties){Copy-Item (Join-Path $Root $f.Name) (Join-Path $cases $f.Name);Require ((Get-FileHash (Join-Path $cases $f.Name)).Hash.ToLowerInvariant()-eq$f.Value) 'Source fixture changed'}
 $config=Get-Content (Join-Path $Root 'config-template.json') -Raw|ConvertFrom-Json;$config.theme='Dark';$config.graphics_renderer='Native';$config.bookmarks=@();$config.default_view_mode='Miller';$config.window_size=@(1000,720);$config.installed_office_previews_enabled=$false;$config.search.include_roots=@($cases);$config.search.exclude_roots=@();$config.search.content_indexing_enabled=$false;$config.search.refresh_interval_hours=0
 $pref=Join-Path $env:APPDATA 'ottrin\config.json';Require (-not(Test-Path $pref)) 'Existing preferences retained';$null=[IO.Directory]::CreateDirectory((Split-Path $pref));[IO.File]::WriteAllText($pref,($config|ConvertTo-Json -Depth 60),[Text.UTF8Encoding]::new($false));$env:OTTRIN_RENDERER=$null;$env:OTTRIN_OFFICE_HELPER=$null;$env:OTTRIN_PDFIUM_DIR=$null
 Phase 'install exact current NSIS';$null=Run (Join-Path $Root 'current-setup.exe') '/S' 'install'
 foreach($f in $manifest.PSObject.Properties){Require ((Get-FileHash (Join-Path $install $f.Name)).Hash.ToLowerInvariant()-eq$f.Value) ('Installed payload changed: '+$f.Name)};Require (@(Get-ChildItem $install -Recurse -File).Count-eq59) 'Unexpected installed payload';$report.payload_files=58
 $report.test_kind='native-worksheet-typography'
 foreach($renderer in @('native','opengl')){
  $suffix=if($renderer-eq'opengl'){' --renderer opengl'}else{''};$case=@{};$report.cases[$renderer]=$case;$file=Join-Path $cases '02-office.xlsx'
  foreach($surface in @('main','inspector')){
   Phase ($renderer+' '+$surface+' native worksheet');$arguments=if($surface-eq'main'){'--select "'+$file+'"'+$suffix}else{'preview "'+$file+'"'+$suffix};$p=Start-Owned $arguments ($renderer+'-'+$surface+'-native');$label=if($surface-eq'main'){'Ottrin'}else{'02-office.xlsx'};$view=Wait-NativeWorksheet $p $label ($renderer+'-'+$surface+'-native');$case[$surface]=@{title=$view.title;expected_native_cell_names=$view.expected_native_cell_names;modules=@(Modules $p $renderer)};Close-Owned $p $view.window
  }
 }
 foreach($f in $packageInput.fixtures.PSObject.Properties){Require ((Get-FileHash (Join-Path $cases $f.Name)).Hash.ToLowerInvariant()-eq$f.Value) 'Source modified by preview'};$report.source_preserved=$true
 Require (@(Get-ChildItem $env:TEMP -Directory -Filter 'ottrin-office-preview-*' -ErrorAction SilentlyContinue).Count-eq0) 'Office temporary staging remains';$report.office_temp_removed=$true
 $prefBefore=(Get-FileHash $pref).Hash;Phase 'normal supported uninstall';$uninstaller=Join-Path $out 'owned-uninstall.exe';Copy-Item (Join-Path $install 'Uninstall.exe') $uninstaller;$null=Run $uninstaller ('/S _?='+$install) 'uninstall';Remove-Item $uninstaller;Require (-not(Test-Path $install)) 'Installed app remains';Require ((Get-FileHash $pref).Hash-eq$prefBefore) 'Removal changed preferences';Require (-not(Test-Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Ottrin')) 'Uninstall registration remains';$report.uninstall='pass';$report.preferences_preserved_by_removal=$true;$final='pass within installed native worksheet scope; direct capture review remains required'
}catch{$report.error=$_.Exception.Message;$report.stack=$_.ScriptStackTrace}
finally{
 foreach($p in $owned){if(-not$p.HasExited){$w=[OfficeUI]::Find($p.Id,'Ottrin');if($w-ne[IntPtr]::Zero){$null=[OfficeUI]::PostMessage($w,0x10,[IntPtr]::Zero,[IntPtr]::Zero)};if(-not$p.WaitForExit(5000)){& "$env:SystemRoot\System32\taskkill.exe" /PID $p.Id /T /F|Out-Null;$null=$p.WaitForExit(10000)}}}
 if($bin-and(Test-Path (Join-Path $bin 'ottrin-office-render.held'))){Rename-Item (Join-Path $bin 'ottrin-office-render.held') 'ottrin-office-render.exe'}
 try{if($app-and(Test-Path $app)){$cleanup=Run $app 'index service-remove' 'service-cleanup';$report.service_cleanup_exit=$cleanup.exit};$deadline=[DateTime]::UtcNow.AddSeconds(15);do{$left=@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath-and$_.ExecutablePath.StartsWith($install,[StringComparison]::OrdinalIgnoreCase)});if(-not$left.Count){break};Start-Sleep -Milliseconds 200}while([DateTime]::UtcNow-lt$deadline);Require ($left.Count-eq0) 'Owned installed process remains';$report.owned_processes_removed=$true}catch{$report.cleanup_error=$_.Exception.Message;$final='failed owned service/process cleanup'}
 $report.status=$final;Save
}
if(-not$final.StartsWith('pass')){exit 1}
