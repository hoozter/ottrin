param([switch]$FreshMsiOnly)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$b='C:\OttrinDesktopQA';$r="$b\results";$result=@{status='running';phase='prepare';scope='Current native standard interactive user; silent installer lifecycle, same-version MSI upgrade, installed OpenGL GUI'}
$app=Join-Path $env:LOCALAPPDATA 'Programs\Ottrin\bin\ottrin.exe';$install=Split-Path(Split-Path $app);$pref=Join-Path $env:APPDATA 'ottrin\config.json';$fixture=Join-Path $env:USERPROFILE 'OttrinFixtures'
$gui=$null;$installedKind=$null;$code=$null
function Assert($c,$m){if(-not $c){throw $m}}
function Phase($name){$result.phase=$name;$result|ConvertTo-Json -Depth 15|Set-Content "$r\status.json"}
function Invoke-Program($path,$arguments,$timeout=120000){
 $p=Start-Process $path -ArgumentList $arguments -PassThru;$null=$p.Handle
 try {Assert ($p.WaitForExit($timeout)) "Owned process still live: $($p.Id)";Assert ($p.ExitCode -eq 0) "$path exit $($p.ExitCode): $arguments";return $p.ExitCode}
 finally {if(-not $p.HasExited){$p.Kill();$null=$p.WaitForExit(5000)};$p.Dispose()}
}
function Invoke-App($arguments){
 $s=New-Object Diagnostics.ProcessStartInfo($script:app,$arguments);$s.UseShellExecute=$false;$s.RedirectStandardOutput=$true;$s.RedirectStandardError=$true
 $p=[Diagnostics.Process]::Start($s)
 try {$out=$p.StandardOutput.ReadToEndAsync();$err=$p.StandardError.ReadToEndAsync();Assert ($p.WaitForExit(45000)) "CLI timeout PID $($p.Id)";Assert ($out.Wait(5000)-and$err.Wait(5000)) 'CLI output EOF timeout';Assert ($p.ExitCode -eq 0) "CLI failed: $($err.Result)";return $out.Result}
 finally {if(-not$p.HasExited){$p.Kill();$null=$p.WaitForExit(5000)};$p.Dispose()}
}
function Live-Index {
 $t=$script:folder.GetTask($script:taskName);Assert ($t.State -eq 4 -and $t.GetInstances(0).Count -eq 1) 'Expected singleton running task'
 $workers=@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue |Where-Object {$_.Path -eq "$script:install\bin\ottrin-indexd.exe"});Assert ($workers.Count -eq 1) 'Expected one installed daemon'
 Assert ($t.Definition.Principal.RunLevel -eq 0) 'Elevated index task';Assert ($t.Definition.Actions.Item(1).Arguments -eq '--index-daemon') 'Wrong index action'
 Assert ($t.Definition.Triggers.Count -eq 1 -and $t.Definition.Triggers.Item(1).Type -eq 9) 'Wrong login trigger'
 return @{pid=$workers[0].Id;instance=$t.GetInstances(0).Item(1).InstanceGuid;task_xml=$t.Xml}
}
function Verify-Payload($msi){
 $m=Get-Content "$b\payload-hashes.json" -Raw|ConvertFrom-Json;$hashes=$m.payload_sha256
 foreach($item in $hashes.PSObject.Properties){Assert ((Get-FileHash (Join-Path $install $item.Name)).Hash.ToLower() -eq $item.Value) "Payload mismatch: $($item.Name)"}
 if($msi){$extra=(Get-Content "$b\msi-hashes.json" -Raw|ConvertFrom-Json).payload_sha256;foreach($item in $extra.PSObject.Properties){Assert ((Get-FileHash (Join-Path $install $item.Name)).Hash.ToLower() -eq $item.Value) "MSI mismatch: $($item.Name)"}}
 return @($hashes.PSObject.Properties).Count
}
function Verify-Removed($expectedPref){
 # NSIS starts a temporary uninstaller; the launcher exit is not completion.
 $deadline=[DateTime]::UtcNow.AddSeconds(45)
 while(((Test-Path $script:install) -or (Test-Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Ottrin')) -and [DateTime]::UtcNow-lt$deadline){Start-Sleep -Milliseconds 200}
 Assert (-not(Test-Path $script:install)) 'Installed folder remains'
 Assert (-not(Test-Path (Join-Path $env:LOCALAPPDATA 'ottrin\search.db'))) 'Index database remains'
 Assert (-not(Test-Path (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Ottrin'))) 'Start menu remains'
 $absent=$false;try{$null=$script:folder.GetTask($script:taskName)}catch{if($_.Exception.HResult -eq -2147024894){$absent=$true}else{throw}}
 Assert $absent 'Index task remains';Assert ((Get-FileHash $script:pref).Hash -eq $expectedPref) 'Uninstall changed preference'
 Assert ((Get-FileHash "$script:fixture\scheduled-daemon-fixture.txt").Hash -eq $script:sourceHash) 'Source fixture changed'
}
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
Add-Type @'
using System;using System.Runtime.InteropServices;
public static class DesktopQA {
 [StructLayout(LayoutKind.Sequential)] public struct RECT {public int Left,Top,Right,Bottom;}
 [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out RECT r);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern void mouse_event(uint f,uint x,uint y,uint d,UIntPtr extra);
}
'@
$null=[DesktopQA]::SetProcessDPIAware()
function Capture($name){
 $script:gui.Refresh();$rect=New-Object DesktopQA+RECT;Assert ([DesktopQA]::GetWindowRect($script:gui.MainWindowHandle,[ref]$rect)) 'No window geometry'
 $bitmap=New-Object Drawing.Bitmap(($rect.Right-$rect.Left),($rect.Bottom-$rect.Top));$g=[Drawing.Graphics]::FromImage($bitmap)
 try {$g.CopyFromScreen($rect.Left,$rect.Top,0,0,$bitmap.Size);$bitmap.Save("$script:r\$name.png",[Drawing.Imaging.ImageFormat]::Png)}finally{$g.Dispose();$bitmap.Dispose()}
 return $rect
}
function Click($x,$y){$null=[DesktopQA]::SetCursorPos($x,$y);[DesktopQA]::mouse_event(2,0,0,0,[UIntPtr]::Zero);[DesktopQA]::mouse_event(4,0,0,0,[UIntPtr]::Zero);Start-Sleep -Milliseconds 300}
 function ProductCode($path){$db=$installer.OpenDatabase($path,0);$v=$db.OpenView("SELECT ``Value`` FROM ``Property`` WHERE ``Property``='ProductCode'");$null=$v.Execute();$value=$v.Fetch().StringData(1);$null=$v.Close();return $value}

try {
 Assert ($env:USERNAME -eq 'OttrinDesktopQA') 'Wrong account';$id=[Security.Principal.WindowsIdentity]::GetCurrent();Assert (-not(New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Elevated actor'
 $result.account_sid=$id.User.Value;$result.session_id=[Diagnostics.Process]::GetCurrentProcess().SessionId;Assert ($result.session_id -gt 0) 'Noninteractive session'
 Assert (-not(Test-Path $install)) 'Unexpected installed application'
 New-Item -ItemType Directory "$fixture\AddressReplacement",(Split-Path $pref) -Force|Out-Null
 [IO.File]::WriteAllText("$fixture\scheduled-daemon-fixture.txt",'fixture original bytes')
 $sourceHash=(Get-FileHash "$fixture\scheduled-daemon-fixture.txt").Hash
 $c=Get-Content "$b\config-template.json" -Raw|ConvertFrom-Json;$c.theme='Dark';$c.default_view_mode='Miller';$c.bookmarks=@();$c.last_session_dirs=@();$c.restore_session=$false;$c.search.include_roots=@($fixture);$c.search.exclude_roots=@();$c.search.use_system_locate=$false;$c.search.manage_locate_db=$false;$c.search.refresh_interval_hours=0
 [IO.File]::WriteAllText($pref,($c|ConvertTo-Json -Depth 30),(New-Object Text.UTF8Encoding($false)))
 $prefHash=(Get-FileHash $pref).Hash;$result.initial_preference_hash=$prefHash;$result.source_hash=$sourceHash
 $scheduler=New-Object -ComObject Schedule.Service;$scheduler.Connect();$folder=$scheduler.GetFolder('\');$taskName='Ottrin Index '+$id.User.Value
 $installer=New-Object -ComObject WindowsInstaller.Installer
 $oldCode=ProductCode "$b\baseline.msi";$code=ProductCode "$b\current.msi";Assert ($oldCode -ne $code) 'Upgrade needs different product codes';$result.old_product_code=$oldCode;$result.product_code=$code
 if(-not $FreshMsiOnly){
 Phase 'nsis-install';$installedKind='nsis';$result.nsis_install=Invoke-Program "$b\setup.exe" '/S'
 $result.nsis_payload_files=Verify-Payload $false;Assert ((Get-FileHash $pref).Hash -eq $prefHash) 'Install changed preferences'
 $result.index_update=Invoke-App 'index update --json';$query=(Invoke-App 'search scheduled-daemon-fixture --json')|ConvertFrom-Json;Assert ($query.total -eq 1) 'Initial query missing'
 $result.before_gui=Live-Index
 Phase 'nsis-gui';$env:OTTRIN_RENDERER='opengl';$env:OTTRIN_DEV_HELPER_AUTOBUILD='0'
 $gui=Start-Process $app -ArgumentList "--select `"$fixture\scheduled-daemon-fixture.txt`"" -PassThru -RedirectStandardOutput "$r\gui.stdout" -RedirectStandardError "$r\gui.stderr";$null=$gui.Handle
 $deadline=[DateTime]::UtcNow.AddSeconds(45)
 do {$gui.Refresh();Assert (-not$gui.HasExited) 'GUI exited early';if($gui.MainWindowHandle -ne 0){break};Start-Sleep -Milliseconds 150}while([DateTime]::UtcNow-lt$deadline)
 Assert ($gui.MainWindowHandle -ne 0) 'No main window';Start-Sleep -Seconds 3;$rect=Capture 'opened';$result.window_title=$gui.MainWindowTitle;$result.window_rect=@{left=$rect.Left;top=$rect.Top;right=$rect.Right;bottom=$rect.Bottom};$wa=[Windows.Forms.Screen]::FromHandle($gui.MainWindowHandle).WorkingArea;$result.work_area=@{left=$wa.Left;top=$wa.Top;right=$wa.Right;bottom=$wa.Bottom}
 $null=[DesktopQA]::SetForegroundWindow($gui.MainWindowHandle);Click ($rect.Left+180) ($rect.Top+17);Assert ([DesktopQA]::GetForegroundWindow() -eq $gui.MainWindowHandle) 'Window not foreground'
 Click ($rect.Left+518) ($rect.Top+407);$null=Capture 'notice-dismissed'
 $width=$rect.Right-$rect.Left
 Click ($rect.Left+$width-27) ($rect.Top+54);$null=Capture 'main-menu';[Windows.Forms.SendKeys]::SendWait('{ESC}')
 Click ($rect.Left+$width-89) ($rect.Top+54);$null=Capture 'view-menu';[Windows.Forms.SendKeys]::SendWait('{ESC}')
 [Windows.Forms.SendKeys]::SendWait('^l');Start-Sleep -Milliseconds 200;[Windows.Forms.Clipboard]::SetText("$fixture\AddressReplacement");[Windows.Forms.SendKeys]::SendWait('^v');[Windows.Forms.SendKeys]::SendWait('{ENTER}');Start-Sleep -Seconds 1
 [Windows.Forms.SendKeys]::SendWait('^l');Start-Sleep -Milliseconds 200;[Windows.Forms.SendKeys]::SendWait('^c');Start-Sleep -Milliseconds 200;$address=[Windows.Forms.Clipboard]::GetText();[Windows.Forms.SendKeys]::SendWait('{ESC}');Assert ($address.TrimEnd('\') -eq "$fixture\AddressReplacement") "Address replacement failed: $address";$result.address_replacement=$address;$null=Capture 'address-replaced'
 [Windows.Forms.SendKeys]::SendWait('^f');Start-Sleep -Milliseconds 300;[Windows.Forms.Clipboard]::SetText('scheduled-daemon-fixture');[Windows.Forms.SendKeys]::SendWait('^v');Start-Sleep -Seconds 1;$null=Capture 'search'
 $result.during_gui=Live-Index;Assert ($result.during_gui.pid -eq $result.before_gui.pid -and $result.during_gui.instance -eq $result.before_gui.instance) 'GUI replaced daemon'
 Assert ($gui.CloseMainWindow()) 'Normal close request failed';Assert ($gui.WaitForExit(15000)) 'GUI close timeout';Assert ($gui.ExitCode -eq 0) 'GUI exit failed';$gui.Dispose();$gui=$null
 $result.after_gui=Live-Index;Assert ($result.after_gui.pid -eq $result.before_gui.pid -and $result.after_gui.instance -eq $result.before_gui.instance) 'Daemon stopped or changed on GUI close'
 [IO.File]::WriteAllText("$fixture\after-window-closed.txt",'watch after close')
 $deadline=[DateTime]::UtcNow.AddSeconds(20);$found=$false
 do {$query=(Invoke-App 'search after-window-closed --json')|ConvertFrom-Json;if($query.total -eq 1){$found=$true;break};Start-Sleep -Milliseconds 250}while([DateTime]::UtcNow-lt$deadline)
 Assert $found 'Watcher stopped after GUI close';$result.watcher_after_gui=$true
 $saved=Get-Content $pref -Raw|ConvertFrom-Json;Assert ($saved.default_view_mode -eq 'Miller'-and$saved.theme -eq 'Dark') 'Saved appearance/view changed'
 $prefHash=(Get-FileHash $pref).Hash
 Phase 'nsis-uninstall';$result.nsis_uninstall=Invoke-Program "$install\Uninstall.exe" '/S';Verify-Removed $prefHash;$installedKind=$null
 Phase 'baseline-msi-install'
 $installedKind='msi-old';$result.baseline_msi_install=Invoke-Program msiexec.exe "/i $b\baseline.msi /qn /norestart /l*v $r\baseline-install.log";Assert ($installer.ProductState($oldCode)-eq 5) 'Baseline not installed';$result.before_upgrade=Live-Index
 }
 if($FreshMsiOnly){$result.scope='Current MSI fresh install/uninstall; native standard interactive user; 53-file identity, index and data preservation';Phase 'msi-fresh-install'}else{Phase 'msi-same-version-upgrade'};$installedKind='msi-current';$logName=if($FreshMsiOnly){'fresh-install.log'}else{'upgrade.log'};$msiExit=Invoke-Program msiexec.exe "/i $b\current.msi /qn /norestart /l*v $r\$logName";if($FreshMsiOnly){$result.msi_install=$msiExit}else{$result.msi_upgrade=$msiExit}
 Assert ($installer.ProductState($oldCode)-eq -1) 'Old MSI product remains';Assert ($installer.ProductState($code)-eq 5) 'Current MSI not installed';$result.msi_payload_files=@((Get-Content "$b\msi-hashes.json" -Raw|ConvertFrom-Json).payload_sha256.PSObject.Properties).Count
 $null=Verify-Payload $true;$result.after_upgrade=Live-Index;$result.upgraded_index=Invoke-App 'index update --json';$query=(Invoke-App 'search scheduled-daemon-fixture --json')|ConvertFrom-Json;Assert ($query.total-eq 1) 'Upgrade query missing';Assert ((Get-FileHash $pref).Hash-eq$prefHash) 'MSI upgrade changed preference'
 Phase 'msi-uninstall';$result.msi_uninstall=Invoke-Program msiexec.exe "/x $code /qn /norestart /l*v $r\uninstall.log";Assert ($installer.ProductState($code)-eq -1) 'MSI product remains';Verify-Removed $prefHash;$installedKind=$null
 $result.final_preference_hash=(Get-FileHash $pref).Hash;$result.final_source_hash=(Get-FileHash "$fixture\scheduled-daemon-fixture.txt").Hash;$result.status='pass';Phase 'complete'
} catch {$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally {
 if($gui){if(-not$gui.HasExited){$gui.Kill();$null=$gui.WaitForExit(5000)};$gui.Dispose()}
 if($result.status -eq 'fail' -and $installedKind){
  try {
   if(Test-Path $app){$null=Invoke-App '--uninstall'}
   if($installedKind-eq'nsis' -and (Test-Path "$install\Uninstall.exe")){$null=Invoke-Program "$install\Uninstall.exe" '/S'}
   elseif($installedKind-like'msi*'){
    foreach($ownedCode in @($code,$oldCode)|Select-Object -Unique){if($ownedCode -and $installer.ProductState($ownedCode)-gt 0){$null=Invoke-Program msiexec.exe "/x $ownedCode /qn /norestart /l*v $r\failure-cleanup.log"}}
   }
   $result.failure_cleanup_attempted=$true
  } catch {$result.failure_cleanup_error=$_.Exception.Message}
 }
 $result.remaining_install_kind=$installedKind;$result|ConvertTo-Json -Depth 20|Set-Content "$r\result.json"
}
if($result.status-ne'pass'){exit 1}
