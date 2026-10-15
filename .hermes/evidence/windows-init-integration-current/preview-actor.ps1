param([ValidateSet('Automatic','OpenGl')][string]$Renderer='Automatic')
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$b='C:\OttrinInitFinalQA';$r="$b\results";$install="$env:LOCALAPPDATA\Programs\Ottrin";$app="$install\bin\ottrin.exe";$pref="$env:APPDATA\ottrin\config.json";$controller="$b\controller.exe"
$result=@{status='running';renderer_request=$Renderer;scope='Exact NSIS installed GUI, ordinary interactive user, read-only preview navigation and source preservation'}
$active=@();$gui=$null;$saved=$null;$installed=$false
function Assert($v,$m){if(-not$v){throw $m}}
function StopTree($p){if($p-and-not$p.HasExited){& "$env:WINDIR\System32\taskkill.exe" /PID $p.Id /T /F|Out-Null;Assert ($LASTEXITCODE-eq0) 'Cannot terminate owned process tree';Assert ($p.WaitForExit(10000)) 'Owned process remains live'}}
function Run($exe,$arguments){$p=Start-Process $exe -ArgumentList $arguments -PassThru;$null=$p.Handle;$script:active+=,$p;Assert ($p.WaitForExit(180000)) "Owned command timeout $exe";Assert ($p.ExitCode-eq0) "Owned command failed $exe ($($p.ExitCode))"}
function Phase($name){$result.phase=$name;$result|ConvertTo-Json -Depth 8|Set-Content "$r\GUI-$Renderer-PROGRESS.json"}
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
Add-Type @'
using System;using System.Text;using System.Collections.Generic;using System.Runtime.InteropServices;
public static class OttrinPreviewGuiQA {
 public class Window {public IntPtr Handle;public string Title;}
 [StructLayout(LayoutKind.Sequential)] public struct RECT {public int Left,Top,Right,Bottom;}
 [StructLayout(LayoutKind.Sequential)] public struct POINT {public int X,Y;}
 delegate bool EnumProc(IntPtr h,IntPtr p);
 [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f,IntPtr p);
 [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h,StringBuilder text,int n);
 [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h,ref POINT p);
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern void mouse_event(uint flags,uint x,uint y,uint data,UIntPtr extra);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
 public static Window[] Windows(int owner) {var list=new List<Window>();EnumWindows((h,p)=>{uint pid;GetWindowThreadProcessId(h,out pid);if(pid==(uint)owner&&IsWindowVisible(h)){var s=new StringBuilder(2048);GetWindowText(h,s,s.Capacity);list.Add(new Window{Handle=h,Title=s.ToString()});}return true;},IntPtr.Zero);return list.ToArray();}
}
'@
$null=[OttrinPreviewGuiQA]::SetProcessDPIAware()
function Windows { @([OttrinPreviewGuiQA]::Windows($script:gui.Id)) }
function WaitWindow($pattern){$deadline=[DateTime]::UtcNow.AddSeconds(40);do{Assert (-not$gui.HasExited) 'GUI exited while waiting for window';$wins=@(Windows|Where-Object Title -match $pattern);if($wins.Count-eq1){return $wins[0]};Start-Sleep -Milliseconds 100}while([DateTime]::UtcNow-lt$deadline);throw "Missing owned GUI window $pattern"}
function Capture($w,$name){$rect=[OttrinPreviewGuiQA+RECT]::new();$origin=[OttrinPreviewGuiQA+POINT]::new();Assert ([OttrinPreviewGuiQA]::GetClientRect($w.Handle,[ref]$rect)) 'Missing client geometry';Assert ([OttrinPreviewGuiQA]::ClientToScreen($w.Handle,[ref]$origin)) 'Missing client origin';$bitmap=[Drawing.Bitmap]::new(($rect.Right-$rect.Left),($rect.Bottom-$rect.Top));$g=[Drawing.Graphics]::FromImage($bitmap);try{$g.CopyFromScreen($origin.X,$origin.Y,0,0,$bitmap.Size);$bitmap.Save("$r\GUI-$Renderer-$name.png",[Drawing.Imaging.ImageFormat]::Png)}finally{$g.Dispose();$bitmap.Dispose()}}
function Focus($w){$null=[OttrinPreviewGuiQA]::SetForegroundWindow($w.Handle);$rect=[OttrinPreviewGuiQA+RECT]::new();Assert ([OttrinPreviewGuiQA]::GetWindowRect($w.Handle,[ref]$rect)) 'Missing focus geometry';$null=[OttrinPreviewGuiQA]::SetCursorPos(($rect.Left+200),($rect.Top+16));[OttrinPreviewGuiQA]::mouse_event(2,0,0,0,[UIntPtr]::Zero);[OttrinPreviewGuiQA]::mouse_event(4,0,0,0,[UIntPtr]::Zero);Start-Sleep -Milliseconds 250;Assert ([OttrinPreviewGuiQA]::GetForegroundWindow()-eq$w.Handle) 'Owned GUI window not foreground'}
function Key($w,$key){Focus $w;[Windows.Forms.SendKeys]::SendWait($key);Start-Sleep -Milliseconds 400}
function LiveIndex {$t=$folder.GetTask($taskName);$procs=@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue|Where-Object Path -eq "$install\bin\ottrin-indexd.exe");Assert ($procs.Count-eq1-and$t.GetInstances(0).Count-eq1) 'Expected one configured daemon';@{pid=$procs[0].Id;instance=$t.GetInstances(0).Item(1).InstanceGuid}}
try{
 Assert ($env:USERNAME-eq'OttrinInitFinalQA') 'Wrong actor account';$id=[Security.Principal.WindowsIdentity]::GetCurrent();Assert (-not(New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Actor is elevated'
 Assert (-not(Test-Path $install)) 'Unexpected installed payload';$saved=[IO.File]::ReadAllBytes($pref)
 $fixture="$env:USERPROFILE\OttrinFixtures\preview-ui";Assert (-not(Test-Path $fixture)) 'Unexpected GUI fixture';New-Item -ItemType Directory $fixture|Out-Null
 $vcf="$fixture\a.vcf";[IO.File]::WriteAllText($vcf,"BEGIN:VCARD`nVERSION:3.0`nFN:Read Only Contact`nEND:VCARD`n",[Text.UTF8Encoding]::new($false));Copy-Item "$b\preview.pdf" "$fixture\b.pdf";$vcfHash=(Get-FileHash $vcf).Hash;$pdfHash=(Get-FileHash "$fixture\b.pdf").Hash
 $config=[Text.Encoding]::UTF8.GetString($saved)|ConvertFrom-Json;$config.theme='Dark';$config.default_view_mode='Miller';$config.window_size=@(1000,700);$config.restore_session=$false;$config.search_setup_completed=$true;$config.graphics_compatibility_notice_seen=$true
 [IO.File]::WriteAllText($pref,($config|ConvertTo-Json -Depth 50),[Text.UTF8Encoding]::new($false))
 Phase 'install';$installed=$true;Run "$b\current.exe" '/S'
 $manifest=Get-Content "$b\payload.json" -Raw|ConvertFrom-Json;Assert ((Get-FileHash $app).Hash.ToLowerInvariant()-ceq$manifest.'bin/ottrin.exe') 'Wrong installed GUI'
 $scheduler=New-Object -ComObject Schedule.Service;$scheduler.Connect();$folder=$scheduler.GetFolder('\');$taskName='Ottrin Index '+$id.User.Value;$result.index_before=LiveIndex
 $env:OTTRIN_DEV_HELPER_AUTOBUILD='0';if($Renderer-eq'OpenGl'){$env:OTTRIN_RENDERER='opengl'}else{Remove-Item Env:\OTTRIN_RENDERER -ErrorAction SilentlyContinue}
 Phase 'launch';$gui=Start-Process $app -ArgumentList ('--select "'+$vcf+'"') -PassThru -RedirectStandardOutput "$r\GUI-$Renderer.stdout" -RedirectStandardError "$r\GUI-$Renderer.stderr";$null=$gui.Handle;$active+=,$gui
 $root=WaitWindow '^Ottrin$';Start-Sleep -Seconds 4;Capture $root 'root';$result.root_dpi=[OttrinPreviewGuiQA]::GetDpiForWindow($root.Handle)
 Phase 'readonly-preview';Key $root ' ';$preview=WaitWindow 'Quick Inspector.*a\.vcf$';Start-Sleep -Seconds 1;Capture $preview 'vcf'
 $result.opened=$preview.Title;Key $preview '{DOWN}';$next=WaitWindow 'Quick Inspector.*b\.pdf$';$result.after_down=$next.Title;Start-Sleep -Seconds 2;Capture $next 'pdf'
 Key $next '{UP}';$back=WaitWindow 'Quick Inspector.*a\.vcf$';$result.after_up=$back.Title;Key $back ' '
 $deadline=[DateTime]::UtcNow.AddSeconds(5);while(@(Windows|Where-Object Title -like '*Quick Inspector*').Count-gt0-and[DateTime]::UtcNow-lt$deadline){Start-Sleep -Milliseconds 100}
 Assert (@(Windows|Where-Object Title -like '*Quick Inspector*').Count-eq0) 'Space did not close read-only Inspector';$result.space_closed=$true
 Assert ((Get-FileHash $vcf).Hash-ceq$vcfHash-and(Get-FileHash "$fixture\b.pdf").Hash-ceq$pdfHash) 'Preview changed source files';$result.source_bytes_unchanged=$true
 $result.graphics_modules=@($gui.Modules|Where-Object ModuleName -in @('opengl32.dll','libgallium_wgl.dll','d3d12.dll','dxgi.dll')|Select-Object ModuleName,FileName)
 $result.index_while_open=LiveIndex;Assert ($result.index_before.pid-eq$result.index_while_open.pid-and$result.index_before.instance-ceq$result.index_while_open.instance) 'GUI replaced configured daemon'
 Phase 'close';$gui.Refresh();$null=$gui.CloseMainWindow();Assert ($gui.WaitForExit(15000)-and$gui.ExitCode-eq0) 'GUI failed normal close';$result.index_after=LiveIndex;Assert ($result.index_before.pid-eq$result.index_after.pid-and$result.index_before.instance-ceq$result.index_after.instance) 'GUI close stopped/replaced configured daemon'
 $result.status='pass';Phase 'accepted'
}catch{$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally{
 $errors=@();foreach($p in $active){try{StopTree $p}catch{$errors+=$_.Exception.Message}}
 try{if($saved){[IO.File]::WriteAllBytes($pref,$saved)};if($installed-and(Test-Path "$install\Uninstall.exe")){Run $controller '--stop';Copy-Item "$install\Uninstall.exe" "$r\GUI-uninstall.exe" -Force;Run "$r\GUI-uninstall.exe" "/S _?=$install";Assert (-not(Test-Path $install)) 'GUI cleanup left payload'};if($fixture-and(Test-Path $fixture)){Remove-Item $fixture -Recurse -Force}}catch{$errors+=$_.Exception.Message}
 if($errors.Count){$result.status='fail';$result.cleanup_errors=$errors}
 $result|ConvertTo-Json -Depth 14|Set-Content "$r\GUI-$Renderer-RESULTS.json"
}
if($result.status-ne'pass'){exit 1}
