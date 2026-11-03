param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExpectedSid)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$out=Join-Path $Root 'results';$r=$out;$owned=[Collections.Generic.List[Diagnostics.Process]]::new();$report=@{status='running';scope='Current installed NSIS temporal PDF, read-only VCF sibling navigation/Space, explicit Edit/Save; ordinary stock-client software VM';cases=@{}};$final='failed native preview safety';$bin=$null
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
function Close-Owned($process,$window){Require ([OfficeUI]::PostMessage($window,0x10,[IntPtr]::Zero,[IntPtr]::Zero)) 'Normal Close refused';Require ($process.WaitForExit(15000)) 'Normal Close timed out';$process.WaitForExit();Require ($process.ExitCode-eq0) 'Normal Close failed'}
function Start-Owned($arguments,$name){$p=Start-Process $app -ArgumentList $arguments -WorkingDirectory $bin -PassThru -RedirectStandardOutput (Join-Path $out ($name+'.stdout')) -RedirectStandardError (Join-Path $out ($name+'.stderr'));$null=$p.Handle;$owned.Add($p);return $p}

Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;using System.Drawing;using System.Drawing.Imaging;using System.Runtime.InteropServices;
public static class PreviewPixels {
 [StructLayout(LayoutKind.Sequential)]struct R{public int l,t,r,b;}
 [DllImport("user32.dll")]static extern bool GetWindowRect(IntPtr h,out R r);
 public static int[] PageBounds(IntPtr h,string path){R r;if(!GetWindowRect(h,out r))throw new Exception("No owned window");using(var b=new Bitmap(r.r-r.l,r.b-r.t,PixelFormat.Format32bppArgb)){using(var g=Graphics.FromImage(b))g.CopyFromScreen(r.l,r.t,0,0,b.Size);if(path.Length>0)b.Save(path,ImageFormat.Png);var d=b.LockBits(new Rectangle(0,0,b.Width,b.Height),ImageLockMode.ReadOnly,PixelFormat.Format32bppArgb);try{var a=new byte[d.Stride*b.Height];Marshal.Copy(d.Scan0,a,0,a.Length);var xs=new int[b.Width];var ys=new int[b.Height];for(int y=0;y<b.Height;y++)for(int x=0;x<b.Width;x++){int n=y*d.Stride+x*4;int lo=Math.Min(a[n],Math.Min(a[n+1],a[n+2]));int hi=Math.Max(a[n],Math.Max(a[n+1],a[n+2]));if(lo>235&&hi-lo<8){xs[x]++;ys[y]++;}}int l=b.Width,t=b.Height,z=0,v=0;for(int x=0;x<b.Width;x++)if(xs[x]>80){l=Math.Min(l,x);z=x+1;}for(int y=0;y<b.Height;y++)if(ys[y]>50){t=Math.Min(t,y);v=y+1;}return l<z&&t<v?new int[]{l,t,z,v}:new int[0];}finally{b.UnlockBits(d);}}}
}
'@
$clock=[Diagnostics.Stopwatch]::StartNew()
function Tree($window){return [Windows.Automation.AutomationElement]::FromHandle($window)}
function Wait-Name($window,$name){$deadline=[DateTime]::UtcNow.AddSeconds(30);do{$condition=[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,$name);$nodes=(Tree $window).FindAll([Windows.Automation.TreeScope]::Descendants,$condition);if($nodes.Count-eq1){return $nodes.Item(0)};Start-Sleep -Milliseconds 100}while([DateTime]::UtcNow-lt$deadline);[PickerNative]::Shot($window,(Join-Path $out 'missing-control.png'));throw ('Expected one native control: '+$name+'; found='+$nodes.Count)}
function Invoke-Node($node){Require $node.Current.IsEnabled 'Native action disabled';$node.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke();Start-Sleep -Milliseconds 200}
function Wait-Window($p,$label){$deadline=[DateTime]::UtcNow.AddSeconds(40);do{$p.Refresh();Require (-not$p.HasExited) 'Application exited before window';$w=[OfficeUI]::Find($p.Id,$label);if($w-ne[IntPtr]::Zero){$screen=[Windows.Forms.Screen]::PrimaryScreen.WorkingArea;$null=[OfficeUI]::MoveWindow($w,20,20,[Math]::Min(1000,$screen.Width-40),[Math]::Min(720,$screen.Height-40),$true);Focus-Owned $w $p;Start-Sleep -Milliseconds 500;return $w};Start-Sleep -Milliseconds 100}while([DateTime]::UtcNow-lt$deadline);throw ('Missing owned window: '+$label)}
function Send($window,$keys){[PickerNative]::Focus($window);[Windows.Forms.SendKeys]::SendWait($keys);Start-Sleep -Milliseconds 200}
function Wait-Title($p,$w,$suffix){$deadline=[DateTime]::UtcNow.AddSeconds(15);do{if(([OfficeUI]::Title($w)).EndsWith($suffix)){return};Start-Sleep -Milliseconds 100}while([DateTime]::UtcNow-lt$deadline);throw ('Inspector did not navigate to '+$suffix+'; actual='+[OfficeUI]::Title($w))}
function Wait-Gone($p,$label){$deadline=[DateTime]::UtcNow.AddSeconds(10);do{if([OfficeUI]::Find($p.Id,$label)-eq[IntPtr]::Zero){return};Start-Sleep -Milliseconds 100}while([DateTime]::UtcNow-lt$deadline);throw ('Inspector did not close: '+$label)}
try{
 $id=[Security.Principal.WindowsIdentity]::GetCurrent();Require ($id.User.Value-eq$ExpectedSid) 'Wrong owned SID';Require (-not([Security.Principal.WindowsPrincipal]::new($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Ordinary token required';Require ([Diagnostics.Process]::GetCurrentProcess().SessionId-gt0) 'Interactive desktop required';$report.identity=@{sid=$ExpectedSid;session=[Diagnostics.Process]::GetCurrentProcess().SessionId;administrator=$false;profile=[Environment]::GetFolderPath('UserProfile')}
 Phase 'waiting for observed owned desktop';$deadline=[DateTime]::UtcNow.AddMinutes(8);while(-not(Test-Path (Join-Path $out 'DESKTOP_READY.json'))){Require ([DateTime]::UtcNow-lt$deadline) 'Owned desktop readiness timeout';Start-Sleep -Milliseconds 200};$owner=Get-Content 'C:\OttrinMsiCleanClientResults\OWNER.json' -Raw|ConvertFrom-Json;$ready=Get-Content (Join-Path $out 'DESKTOP_READY.json') -Raw|ConvertFrom-Json;Require ($ready.sid-eq$ExpectedSid-and$ready.nonce-eq$owner.nonce) 'Wrong native desktop observation';$report.desktop_ready=$true
 $packageInput=Get-Content (Join-Path $Root 'application-input.json') -Raw|ConvertFrom-Json;foreach($f in $packageInput.files){Require ((Get-FileHash (Join-Path $Root $f.name)).Hash.ToLowerInvariant()-eq$f.sha256) 'Package input changed'};$report.package=$packageInput
 $manifest=Get-Content (Join-Path $Root 'payload.json') -Raw|ConvertFrom-Json;$install=Join-Path $env:LOCALAPPDATA 'Programs\Ottrin';$bin=Join-Path $install 'bin';$app=Join-Path $bin 'ottrin.exe';Require (-not(Test-Path $install)) 'Existing owned-account install retained'
 $cases=Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'OttrinOwnedPreview';Require (-not(Test-Path $cases)) 'Existing source retained';$null=New-Item -ItemType Directory $cases
 $vcf=Join-Path $cases 'a.vcf';$original="BEGIN:VCARD`nVERSION:3.0`nFN:Read Only Contact`nEND:VCARD`n";[IO.File]::WriteAllText($vcf,$original,[Text.UTF8Encoding]::new($false))
 [IO.File]::WriteAllText((Join-Path $cases 'c.txt'),"Other read-only text`n",[Text.UTF8Encoding]::new($false))
 $objects=@('<< /Type /Catalog /Pages 2 0 R >>','<< /Type /Pages /Kids [3 0 R 6 0 R] /Count 2 >>','<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>','<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>')
 $stream='BT /F1 24 Tf 60 700 Td (PAGE ONE) Tj ET';$objects+=('<< /Length '+$stream.Length+" >>`nstream`n"+$stream+"`nendstream")
 $objects+='<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Resources << /Font << /F1 4 0 R >> >> /Contents 7 0 R >>';$stream='BT /F1 24 Tf 60 700 Td (PAGE TWO) Tj ET';$objects+=('<< /Length '+$stream.Length+" >>`nstream`n"+$stream+"`nendstream")
 $pdf="%PDF-1.4`n";$offsets=@(0);for($i=0;$i-lt$objects.Count;$i++){$offsets+=$pdf.Length;$pdf+=($i+1).ToString()+" 0 obj`n"+$objects[$i]+"`nendobj`n"};$xref=$pdf.Length;$pdf+="xref`n0 8`n0000000000 65535 f `n";foreach($offset in $offsets[1..7]){$pdf+=$offset.ToString('D10')+" 00000 n `n"};$pdf+="trailer`n<< /Size 8 /Root 1 0 R >>`nstartxref`n"+$xref+"`n%%EOF`n";[IO.File]::WriteAllBytes((Join-Path $cases 'b.pdf'),[Text.Encoding]::ASCII.GetBytes($pdf))
 $before=@{};foreach($f in Get-ChildItem $cases -File){$before[$f.Name]=(Get-FileHash $f.FullName).Hash};$report.sources_before=$before
 $config=Get-Content (Join-Path $Root 'config-template.json') -Raw|ConvertFrom-Json;$config.theme='Dark';$config.graphics_renderer='Native';$config.bookmarks=@();$config.default_view_mode='Miller';$config.window_size=@(1000,720);$config.installed_office_previews_enabled=$false;$config.search.include_roots=@($cases);$config.search.exclude_roots=@();$config.search.content_indexing_enabled=$false;$config.search.refresh_interval_hours=0
 $pref=Join-Path $env:APPDATA 'ottrin\config.json';Require (-not(Test-Path $pref)) 'Existing preferences retained';$null=[IO.Directory]::CreateDirectory((Split-Path $pref));[IO.File]::WriteAllText($pref,($config|ConvertTo-Json -Depth 60),[Text.UTF8Encoding]::new($false));$env:OTTRIN_RENDERER=$null;$env:OTTRIN_OFFICE_HELPER=$null;$env:OTTRIN_PDFIUM_DIR=$null
 Phase 'install exact current NSIS';$null=Run (Join-Path $Root 'current-setup.exe') '/S' 'install'
 foreach($f in $manifest.PSObject.Properties){Require ((Get-FileHash (Join-Path $install $f.Name)).Hash.ToLowerInvariant()-eq$f.Value) ('Installed payload changed: '+$f.Name)};Require (@(Get-ChildItem $install -Recurse -File).Count-eq59) 'Unexpected installed payload';$report.payload_files=58
 $report.test_kind='native-windows-preview-safety'
 Phase 'main VCF to PDF transition';$p=Start-Owned ('--select "'+$vcf+'" --renderer opengl') 'main';$window=Wait-Window $p 'Ottrin';$tree=Tree $window;Wait-Name $window 'Expand preview'|Out-Null
 Send $window '{DOWN}';$fit=Wait-Name $window 'Fit width';Invoke-Node $fit;Start-Sleep -Milliseconds 700
 Send $window '{UP}';Start-Sleep -Milliseconds 300;Send $window '{DOWN}'
 $frames=@();for($i=0;$i-lt100;$i++){Start-Sleep -Milliseconds 70;$path=if($i-in@(0,10,30,99)){Join-Path $out ('pdf-frame-'+$i+'.png')}else{''};$frames+=@{frame=$i;elapsed_ms=$clock.ElapsedMilliseconds;bounds=@([PreviewPixels]::PageBounds($window,$path))}};$report.pdf_frames=$frames;Save
 $settled=@($frames[30..99]|Where-Object {$_.bounds.Count-eq4});Require ($settled.Count-ge60) 'PDF missing in settled temporal samples';$distinct=@($settled|ForEach-Object {$_.bounds-join','}|Select-Object -Unique);Require ($distinct.Count-eq1) 'PDF still changes size after settling';$report.pdf_stable=@{samples=$settled.Count;bounds=$distinct[0];interval_ms=70;scope='Real screen bright-page geometry; frames0/10/30/99 retained for direct review'}
 Send $window '{UP}';Start-Sleep -Milliseconds 400;Send $window ' ';$inspector=Wait-Window $p 'a.vcf';Wait-Name $inspector 'Edit text'|Out-Null;[PickerNative]::Shot($inspector,(Join-Path $out 'vcf-read-only.png'))
 Send $inspector '{DOWN}';Wait-Title $p $inspector 'b.pdf';Wait-Name $inspector 'Fit width'|Out-Null;Send $inspector '{UP}';Wait-Title $p $inspector 'a.vcf';Wait-Name $inspector 'Edit text'|Out-Null;Send $inspector ' ';Wait-Gone $p 'a.vcf';Require ([IO.File]::ReadAllText($vcf)-ceq$original) 'Read-only VCF changed';$report.read_only_navigation=@{next='b.pdf';previous='a.vcf';space_closed=$true;source_unchanged=$true}
 Focus-Owned $window $p;Send $window ' ';$inspector=Wait-Window $p 'a.vcf';$edit=Wait-Name $inspector 'Edit text';Invoke-Node $edit;Wait-Name $inspector 'Save'|Out-Null
 Send $inspector '^{END}';Send $inspector 'EXPLICIT_EDIT_MARKER';Start-Sleep -Milliseconds 400;Require ([IO.File]::ReadAllText($vcf)-ceq$original) 'Draft modified source before Save';[PickerNative]::Shot($inspector,(Join-Path $out 'edit-before-save.png'));Send $inspector '^s'
 $deadline=[DateTime]::UtcNow.AddSeconds(10);while([IO.File]::ReadAllText($vcf)-ceq$original){Require ([DateTime]::UtcNow-lt$deadline) 'Explicit Save did not publish draft';Start-Sleep -Milliseconds 100};$expected=$original+'EXPLICIT_EDIT_MARKER';Require ([IO.File]::ReadAllText($vcf)-ceq$expected) 'Saved bytes differ from exact intended edit';$report.explicit_edit=@{draft_preserved_source=$true;save_exact_bytes=$true;before_sha256=$before['a.vcf'];after_sha256=(Get-FileHash $vcf).Hash};[PickerNative]::Shot($inspector,(Join-Path $out 'edit-after-save.png'));Send $inspector '{ESC}';Wait-Gone $p 'a.vcf';Close-Owned $p $window
 foreach($f in Get-ChildItem $cases -File){if($f.Name-ne'a.vcf'){Require ((Get-FileHash $f.FullName).Hash-eq$before[$f.Name]) 'Unrelated source changed'}};$report.unrelated_sources_preserved=$true
 $prefBefore=(Get-FileHash $pref).Hash;Phase 'normal supported uninstall';$uninstaller=Join-Path $out 'owned-uninstall.exe';Copy-Item (Join-Path $install 'Uninstall.exe') $uninstaller;$null=Run $uninstaller ('/S _?='+$install) 'uninstall';Remove-Item $uninstaller;Require (-not(Test-Path $install)) 'Installed app remains';Require ((Get-FileHash $pref).Hash-eq$prefBefore) 'Removal changed preferences';Require (-not(Test-Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Ottrin')) 'Uninstall registration remains';$report.uninstall='pass';$report.preferences_preserved_by_removal=$true;$final='pass within native Windows preview safety scope; direct captures require review'
}catch{$report.error=$_.Exception.Message;$report.stack=$_.ScriptStackTrace}
finally{
 foreach($p in $owned){if(-not$p.HasExited){$w=[OfficeUI]::Find($p.Id,'Ottrin');if($w-ne[IntPtr]::Zero){$null=[OfficeUI]::PostMessage($w,0x10,[IntPtr]::Zero,[IntPtr]::Zero)};if(-not$p.WaitForExit(5000)){& "$env:SystemRoot\System32\taskkill.exe" /PID $p.Id /T /F|Out-Null;$null=$p.WaitForExit(10000)}}}
 if($bin-and(Test-Path (Join-Path $bin 'ottrin-office-render.held'))){Rename-Item (Join-Path $bin 'ottrin-office-render.held') 'ottrin-office-render.exe'}
 try{if($app-and(Test-Path $app)){$cleanup=Run $app 'index service-remove' 'service-cleanup';$report.service_cleanup_exit=$cleanup.exit};$deadline=[DateTime]::UtcNow.AddSeconds(15);do{$left=@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath-and$_.ExecutablePath.StartsWith($install,[StringComparison]::OrdinalIgnoreCase)});if(-not$left.Count){break};Start-Sleep -Milliseconds 200}while([DateTime]::UtcNow-lt$deadline);Require ($left.Count-eq0) 'Owned installed process remains';$report.owned_processes_removed=$true}catch{$report.cleanup_error=$_.Exception.Message;$final='failed owned service/process cleanup'}
 $report.status=$final;Save
}
if(-not$final.StartsWith('pass')){exit 1}
