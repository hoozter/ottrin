param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExpectedSid)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$out=Join-Path $Root 'results';$report=@{status='running';named_fields=@();scope='Exact installed Windows main Settings and Inspector window radius0/32 comparison; captures and native region report, not acceptance of actual theme compliance';cases=@()};$final='failed native main field audit';$owned=[Collections.Generic.List[Diagnostics.Process]]::new();$forms=[Collections.Generic.List[object]]::new();$install=$null
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
[DllImport("gdi32.dll")]static extern IntPtr CreateRectRgn(int a,int b,int c,int d);
[DllImport("user32.dll")]static extern int GetWindowRgn(IntPtr w,IntPtr r);
[DllImport("gdi32.dll")]static extern bool PtInRegion(IntPtr r,int x,int y);
[DllImport("gdi32.dll")]static extern bool DeleteObject(IntPtr r);
public static string Region(IntPtr w){var r=CreateRectRgn(0,0,0,0);try{int kind=GetWindowRgn(w,r);return "kind="+kind+" point1="+PtInRegion(r,1,1)+" point12="+PtInRegion(r,12,12);}finally{DeleteObject(r);}}
}
'@
 function Await($pidValue,$title){$deadline=[DateTime]::UtcNow.AddSeconds(25);do{$v=[PickerNative]::Window($pidValue,$title);if($v-ne[IntPtr]::Zero){return $v};Pump}while([DateTime]::UtcNow-lt$deadline);throw ('Missing owned window '+$title)}
 function Capture($handle,$label,$radius){[PickerNative]::Place($handle);[PickerNative]::Focus($handle);Start-Sleep -Milliseconds 1000;$report.cases+=@{case=$label;radius=$radius;geometry=[PickerNative]::Geometry($handle);native_region=[RadiusProbe]::Region($handle);capture=($label+'-'+$radius+'.png')};[PickerNative]::Shot($handle,(Join-Path $out ($label+'-'+$radius+'.png')));Save}
 foreach($radius in @(0,32)){
  Phase ('radius '+$radius);$config.theme='Dark';$config.active_custom_theme='Radius audit';$config.custom_themes=@(@{name='Radius audit';base_preset='Ottrin';dark=@{app_radius=$radius};light=@{app_radius=$radius}})
  [IO.File]::WriteAllText($pref,($config|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false))
  $s=[Diagnostics.ProcessStartInfo]::new($app,('"'+$corpus+'"'));$s.UseShellExecute=$false;$p=[Diagnostics.Process]::Start($s);$owned.Add($p);$w=Await $p.Id 'Ottrin';Capture $w 'main' $radius
  Invoke $w 'Menu';Invoke $w ('Settings'+[char]0x2026);$settings=Await $p.Id 'Ottrin Settings';Capture $settings 'settings' $radius;Invoke $settings 'Close window';Start-Sleep -Milliseconds 500
  $row=Node $w 'preserve.txt' ([Windows.Automation.ControlType]::Button);$toggle=$row.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern);if($toggle.Current.ToggleState-ne[Windows.Automation.ToggleState]::On){$toggle.Toggle()};$deadline=[DateTime]::UtcNow.AddSeconds(10);do{Pump;$row=Node $w 'preserve.txt' ([Windows.Automation.ControlType]::Button);$state=$row.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern).Current.ToggleState}while($state-ne[Windows.Automation.ToggleState]::On-and[DateTime]::UtcNow-lt$deadline);Require ($state-eq[Windows.Automation.ToggleState]::On) 'File selection failed'
  Invoke $w 'Menu';Invoke $w 'Quick Inspector Selection';$inspector=Await $p.Id ('Ottrin Quick Inspector '+[char]0x2014+' preserve.txt');Capture $inspector 'inspector' $radius;Invoke $inspector 'Close window';Start-Sleep -Milliseconds 500
  Invoke $w 'Close window';Require ($p.WaitForExit(15000)-and$p.ExitCode-eq0) 'Native main close failed';Require ((Get-FileHash $source).Hash-eq$sourceHash) 'Source changed';$savedConfig=Get-Content $pref -Raw|ConvertFrom-Json;Require ($savedConfig.custom_themes[0].dark.app_radius-eq$radius) 'Configured radius changed';Save
 }
 $saved=(Get-FileHash $pref).Hash;Phase 'supported uninstall';$uninstaller=Join-Path $out 'owned-uninstall.exe';Copy-Item (Join-Path $install 'Uninstall.exe') $uninstaller;$null=Run $uninstaller ('/S _?='+$install) 'uninstall';Remove-Item $uninstaller;Require (-not(Test-Path $install)) 'Installation remains';Require ((Get-FileHash $pref).Hash-eq$saved) 'Uninstall changed preferences';Require ((Get-FileHash $source).Hash-eq$sourceHash) 'Source changed';$report.preferences_preserved=$true;$report.source_preserved=$true;$final='pass native installed radius comparison execution; captures require review and compliance assessment'
}catch{$report.error=$_.Exception.Message;$report.stack=$_.ScriptStackTrace}
finally{
 foreach($p in $owned){if(-not$p.HasExited){& "$env:SystemRoot\System32\taskkill.exe" /PID $p.Id /T /F|Out-Null;$null=$p.WaitForExit(10000)}}
 if($install-and(Test-Path $app)-and$manifest-and(Get-FileHash $app).Hash.ToLowerInvariant()-eq$manifest.'bin/ottrin.exe'){try{$report.cleanup_index_remove=Run $app 'index service-remove' 'cleanup-index-remove'}catch{$report.cleanup_index_error=$_.Exception.Message;$final='failed owned index teardown'}}
 if($install){$left=@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath-and$_.ExecutablePath.StartsWith($install,[StringComparison]::OrdinalIgnoreCase)});$report.remaining_owned_installed_processes=$left.Count;if($left.Count){$final='failed owned process teardown'}}
 $report.status=$final;Save
}
if(-not$final.StartsWith('pass')){exit 1}
