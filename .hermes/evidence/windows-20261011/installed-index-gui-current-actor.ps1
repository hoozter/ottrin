param(
 [Parameter(Mandatory=$true)][ValidatePattern('^[a-f0-9]{64}$')][string]$ExpectedInstallerSha256,
 [Parameter(Mandatory=$true)][ValidatePattern('^[a-f0-9]{64}$')][string]$ExpectedAppSha256,
 [Parameter(Mandatory=$true)][ValidatePattern('^[a-f0-9]{64}$')][string]$ExpectedDaemonSha256
)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$b='C:\OttrinQA-20261011';$r="$b\installed-index-gui-current";$result=@{status='running';scope='Installed NSIS; standard interactive user; bundled OpenGL'}
$install=Join-Path $env:LOCALAPPDATA 'Programs\Ottrin';$app="$install\bin\ottrin.exe";$pref=Join-Path $env:APPDATA 'ottrin\config.json';$fixture=Join-Path $env:USERPROFILE 'OttrinFixtures';$after="$fixture\after-window-closed-current.txt"
$p=$null;$saved=$null;$created=$false;$installed=$false;$installationStarted=$false;$literalDirectories=@()
function Assert($ok,$message){if(-not $ok){throw $message}}
function Invoke-App($arguments){
 $start=[Diagnostics.ProcessStartInfo]::new($script:app,$arguments);$start.UseShellExecute=$false;$start.RedirectStandardOutput=$true;$start.RedirectStandardError=$true;$cli=[Diagnostics.Process]::Start($start)
 try{$out=$cli.StandardOutput.ReadToEndAsync();$err=$cli.StandardError.ReadToEndAsync();Assert ($cli.WaitForExit(30000)) 'CLI timeout';Assert ($cli.ExitCode -eq 0) "CLI failure: $($err.Result)";return $out.Result}
 finally{if(-not$cli.HasExited){$cli.Kill();$null=$cli.WaitForExit(5000)};$cli.Dispose()}
}
function Live-Index {
 $t=$script:folder.GetTask($script:taskName);Assert ($t.State -eq 4 -and $t.GetInstances(0).Count -eq 1) 'Index task not one running instance'
 $procs=@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue|Where-Object {$_.Path -eq "$script:install\bin\ottrin-indexd.exe"});Assert ($procs.Count -eq 1) 'Expected one installed daemon'
 return @{pid=$procs[0].Id;instance=$t.GetInstances(0).Item(1).InstanceGuid}
}
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
Add-Type @'
using System;using System.Runtime.InteropServices;
public static class InstalledGuiQA {
 [StructLayout(LayoutKind.Sequential)] public struct RECT {public int Left,Top,Right,Bottom;}
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] public static extern bool CreateDirectoryW(string path,IntPtr attributes);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] public static extern bool RemoveDirectoryW(string path);
 [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern void mouse_event(uint f,uint x,uint y,uint d,UIntPtr extra);
}
'@
$null=[InstalledGuiQA]::SetProcessDPIAware()
function Capture($process,$name){
 $rect=[InstalledGuiQA+RECT]::new();Assert ([InstalledGuiQA]::GetWindowRect($process.MainWindowHandle,[ref]$rect)) 'No window geometry'
 $bitmap=[Drawing.Bitmap]::new($rect.Right-$rect.Left,$rect.Bottom-$rect.Top);$g=[Drawing.Graphics]::FromImage($bitmap)
 try{$g.CopyFromScreen($rect.Left,$rect.Top,0,0,$bitmap.Size);$bitmap.Save("$script:r\$name.png",[Drawing.Imaging.ImageFormat]::Png)}finally{$g.Dispose();$bitmap.Dispose()}
 return $rect
}
function Click($x,$y){
 $null=[InstalledGuiQA]::SetCursorPos($x,$y);[InstalledGuiQA]::mouse_event(2,0,0,0,[UIntPtr]::Zero);[InstalledGuiQA]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
}
function Address-Value {
 [Windows.Forms.SendKeys]::SendWait('^l');Start-Sleep -Milliseconds 250;[Windows.Forms.SendKeys]::SendWait('^a');[Windows.Forms.SendKeys]::SendWait('^c');Start-Sleep -Milliseconds 200
 $text=[Windows.Forms.Clipboard]::GetText();[Windows.Forms.SendKeys]::SendWait('{ESC}');return $text
}
function Navigate-Address($path){
 [Windows.Forms.SendKeys]::SendWait('^l');Start-Sleep -Milliseconds 200;[Windows.Forms.Clipboard]::SetText($path);[Windows.Forms.SendKeys]::SendWait('^a');[Windows.Forms.SendKeys]::SendWait('^v');[Windows.Forms.SendKeys]::SendWait('{ENTER}');Start-Sleep -Seconds 1
}

try {
 Assert ($env:USERNAME -eq 'OttrinIndexQA') 'Wrong account';$id=[Security.Principal.WindowsIdentity]::GetCurrent();Assert (-not([Security.Principal.WindowsPrincipal]::new($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Elevated actor'
 Assert (-not(Test-Path $install)) 'Unexpected installed app';Assert (-not(Test-Path $after)) 'Unexpected after-close fixture';$saved=[IO.File]::ReadAllBytes($pref);$result.preference_hash_before=(Get-FileHash $pref).Hash;$source="$fixture\scheduled-daemon-fixture.txt";$result.source_hash_before=(Get-FileHash $source).Hash
 $result.installer_sha256=(Get-FileHash "$b\installed-gui-installer.exe").Hash.ToLowerInvariant();Assert ($result.installer_sha256 -eq $ExpectedInstallerSha256) 'Wrong installer'
 $installationStarted=$true;$setup=Start-Process "$b\installed-gui-installer.exe" -ArgumentList '/S' -PassThru;$null=$setup.Handle;Assert ($setup.WaitForExit(120000) -and $setup.ExitCode -eq 0) 'Install failed';$installed=$true
 $result.app_sha256=(Get-FileHash $app).Hash.ToLowerInvariant();Assert ($result.app_sha256 -eq $ExpectedAppSha256) 'Wrong installed GUI'
 $result.daemon_sha256=(Get-FileHash "$install\bin\ottrin-indexd.exe").Hash.ToLowerInvariant();Assert ($result.daemon_sha256 -eq $ExpectedDaemonSha256) 'Wrong daemon'
 $scheduler=New-Object -ComObject Schedule.Service;$scheduler.Connect();$folder=$scheduler.GetFolder('\');$taskName='Ottrin Index '+$id.User.Value
 $null=Invoke-App 'index update --json';$result.before=Live-Index
 $env:OTTRIN_RENDERER='opengl';$env:OTTRIN_DEV_HELPER_AUTOBUILD='0'
 $p=Start-Process $app -ArgumentList "--select $source" -PassThru -RedirectStandardOutput "$r\app.stdout" -RedirectStandardError "$r\app.stderr";$null=$p.Handle
 $deadline=[DateTime]::UtcNow.AddSeconds(40)
 do{$p.Refresh();Assert (-not$p.HasExited) 'GUI exited before window';if($p.MainWindowHandle -ne 0){break};Start-Sleep -Milliseconds 100}while([DateTime]::UtcNow -lt $deadline)
 Assert ($p.MainWindowHandle -ne 0) 'GUI window timeout';Start-Sleep -Seconds 4;$p.Refresh();$rect=Capture $p 'opened'
 $result.gui_pid=$p.Id;$result.title=$p.MainWindowTitle;$result.open=Live-Index;Assert ($result.open.pid -eq $result.before.pid -and $result.open.instance -eq $result.before.instance) 'GUI replaced configured daemon'
 $null=[InstalledGuiQA]::SetForegroundWindow($p.MainWindowHandle);Start-Sleep -Milliseconds 200;$null=[InstalledGuiQA]::SetCursorPos($rect.Left+200,$rect.Top+16);[InstalledGuiQA]::mouse_event(2,0,0,0,[UIntPtr]::Zero);[InstalledGuiQA]::mouse_event(4,0,0,0,[UIntPtr]::Zero);Start-Sleep -Milliseconds 200
 Assert ([InstalledGuiQA]::GetForegroundWindow() -eq $p.MainWindowHandle) 'GUI not foreground'
 Click ($rect.Left+166) ($rect.Top+55);Start-Sleep -Seconds 1;$null=Capture $p 'root-click'
 $result.root_address=Address-Value;Assert (@('C:\','\\?\C:\') -contains $result.root_address) 'Drive breadcrumb did not navigate to the rooted drive'
 $null=Capture $p 'root'
 $decoyRaw='\\?\'+$r+'\literal target';$literalRaw=$decoyRaw+' '
 foreach($dir in @($decoyRaw,$literalRaw)){Assert ([InstalledGuiQA]::CreateDirectoryW($dir,[IntPtr]::Zero)) "Could not create owned literal directory; Win32 $([Runtime.InteropServices.Marshal]::GetLastWin32Error())";$literalDirectories+=,$dir}
 Navigate-Address $literalRaw;$result.literal_address=Address-Value;Assert ($result.literal_address -ceq $literalRaw) 'Address submission trimmed or redirected the literal directory'
 $null=Capture $p 'literal-target'
 Navigate-Address $fixture;$result.returned_address=Address-Value;Assert (@($fixture,('\\?\'+$fixture)) -contains $result.returned_address) 'Could not return to original fixture'
 [Windows.Forms.SendKeys]::SendWait('^f');Start-Sleep -Milliseconds 300;Set-Clipboard 'scheduled-daemon-fixture';[Windows.Forms.SendKeys]::SendWait('^a');[Windows.Forms.SendKeys]::SendWait('^v');Start-Sleep -Seconds 2;$null=Capture $p 'search'
 $result.search=(Invoke-App 'search scheduled-daemon-fixture --json')|ConvertFrom-Json;Assert ($result.search.total -eq 1) 'Expected one fixture result'
 $result.graphics_modules=@($p.Modules|Where-Object ModuleName -in @('opengl32.dll','libgallium_wgl.dll')|Select-Object ModuleName,FileName)
 $null=$p.CloseMainWindow();Assert ($p.WaitForExit(15000) -and $p.ExitCode -eq 0) 'GUI did not close normally';$result.gui_exit=$p.ExitCode
 $result.closed=Live-Index;Assert ($result.closed.pid -eq $result.before.pid -and $result.closed.instance -eq $result.before.instance) 'GUI close stopped/replaced background daemon'
 [IO.File]::WriteAllText($after,'Created after installed GUI closed');$created=$true;$clock=[Diagnostics.Stopwatch]::StartNew();$deadline=[DateTime]::UtcNow.AddSeconds(15)
 do{$q=(Invoke-App 'search after-window-closed-current --json')|ConvertFrom-Json;if($q.total -eq 1){break};Start-Sleep -Milliseconds 100}while([DateTime]::UtcNow -lt $deadline)
 Assert ($q.total -eq 1) 'After-close watch not working';$result.after_close_query=$q;$result.after_close_visibility_ms=$clock.Elapsed.TotalMilliseconds;$result.after_watch=Live-Index
 Assert ($result.after_watch.pid -eq $result.before.pid -and $result.after_watch.instance -eq $result.before.instance) 'Query replaced background daemon'
 $result.preference_hash_after_gui=(Get-FileHash $pref).Hash;$result.view_preference=([IO.File]::ReadAllText($pref)|ConvertFrom-Json).default_view_mode
 $result.status='pass'
}catch{$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally {
 try{
  if($p -and -not$p.HasExited){$p.Kill();Assert ($p.WaitForExit(5000)) 'GUI teardown failed'}
  foreach($dir in $literalDirectories){Assert ([InstalledGuiQA]::RemoveDirectoryW($dir)) "Literal directory cleanup failed; Win32 $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"}
  if($created){Remove-Item $after -Force}
  if($saved){[IO.File]::WriteAllBytes($pref,$saved);$result.preference_hash_restored=(Get-FileHash $pref).Hash;Assert ($result.preference_hash_restored -eq $result.preference_hash_before) 'QA preference restoration failed'}
  if($installationStarted -and (Test-Path "$install\Uninstall.exe")){$un=Start-Process "$install\Uninstall.exe" -ArgumentList '/S' -PassThru;$null=$un.Handle;Assert ($un.WaitForExit(30000) -and $un.ExitCode -eq 0) 'Uninstall bootstrap failed';$deadline=[DateTime]::UtcNow.AddSeconds(30);while((Test-Path $install) -and [DateTime]::UtcNow -lt $deadline){Start-Sleep -Milliseconds 100};Assert (-not(Test-Path $install)) 'Installed app remains';Assert (-not(Test-Path (Join-Path $env:LOCALAPPDATA 'ottrin\search.db'))) 'Generated index remains';$result.uninstalled=$true}
  $result.source_hash_after=(Get-FileHash "$fixture\scheduled-daemon-fixture.txt").Hash;Assert ($result.source_hash_after -eq $result.source_hash_before) 'Source changed'
 }catch{$result.status='fail';$result.cleanup_error=$_.Exception.Message}
}
$result|ConvertTo-Json -Depth 20|Set-Content "$r\result.json" -Encoding UTF8
if($result.status -ne 'pass'){exit 1};exit 0
