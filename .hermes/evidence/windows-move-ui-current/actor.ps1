param([switch]$CrossOnly)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$b='C:\OttrinMoveUIQA';$r="$b\results";$install="$env:LOCALAPPDATA\Programs\Ottrin";$app="$install\bin\ottrin.exe";$pref="$env:APPDATA\ottrin\config.json"
$active=@();$gui=$null;$installed=$false;$result=@{status='running';phase='prepare';cases=@();scope='Exact current NSIS, ordinary user, real GUI Cut/Paste and conflict choices on owned NTFS volumes'}
function Assert($v,$m){if(-not$v){throw $m}}
function StopTree($p){if($p-and-not$p.HasExited){& "$env:WINDIR\System32\taskkill.exe" /PID $p.Id /T /F|Out-Null;Assert ($LASTEXITCODE-eq0) 'Owned process teardown failed';Assert ($p.WaitForExit(10000)) 'Owned process remains'}}
function Run($exe,$arguments){$p=Start-Process $exe -ArgumentList $arguments -PassThru;$null=$p.Handle;$script:active+=,$p;Assert ($p.WaitForExit(180000)) 'Owned command timeout';Assert ($p.ExitCode-eq0) "Command failed $exe ($($p.ExitCode))"}
function Phase($name){$result.phase=$name;$result|ConvertTo-Json -Depth 12|Set-Content "$r\PROGRESS.json"}
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
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h,int command);
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
function Capture($w,$name){$rect=[OttrinPreviewGuiQA+RECT]::new();$origin=[OttrinPreviewGuiQA+POINT]::new();Assert ([OttrinPreviewGuiQA]::GetClientRect($w.Handle,[ref]$rect)) 'Missing client geometry';Assert ([OttrinPreviewGuiQA]::ClientToScreen($w.Handle,[ref]$origin)) 'Missing client origin';$bitmap=[Drawing.Bitmap]::new(($rect.Right-$rect.Left),($rect.Bottom-$rect.Top));$g=[Drawing.Graphics]::FromImage($bitmap);try{$g.CopyFromScreen($origin.X,$origin.Y,0,0,$bitmap.Size);$bitmap.Save("$r\MOVE-$name.png",[Drawing.Imaging.ImageFormat]::Png)}finally{$g.Dispose();$bitmap.Dispose()}}
function Focus($w){
 if([OttrinPreviewGuiQA]::GetForegroundWindow()-eq$w.Handle){return}
 $null=[OttrinPreviewGuiQA]::ShowWindow($w.Handle,9);$deadline=[DateTime]::UtcNow.AddSeconds(5)
 do{$null=[OttrinPreviewGuiQA]::SetForegroundWindow($w.Handle);$rect=[OttrinPreviewGuiQA+RECT]::new();Assert ([OttrinPreviewGuiQA]::GetWindowRect($w.Handle,[ref]$rect)) 'Missing focus geometry';$null=[OttrinPreviewGuiQA]::SetCursorPos(($rect.Left+200),($rect.Top+16));[OttrinPreviewGuiQA]::mouse_event(2,0,0,0,[UIntPtr]::Zero);[OttrinPreviewGuiQA]::mouse_event(4,0,0,0,[UIntPtr]::Zero);Start-Sleep -Milliseconds 250;if([OttrinPreviewGuiQA]::GetForegroundWindow()-eq$w.Handle){return}}while([DateTime]::UtcNow-lt$deadline)
 Capture $w 'focus-failure';throw ('Owned GUI not foreground; expected '+$w.Handle+' actual '+[OttrinPreviewGuiQA]::GetForegroundWindow())
}
function Key($w,$key){Focus $w;[Windows.Forms.SendKeys]::SendWait($key);Start-Sleep -Milliseconds 400}

function FileId($path){$id=[regex]::Match((& fsutil file queryfileid $path|Out-String),'0x[0-9a-fA-F]+').Value;Assert ($id.Length-gt10) 'Missing file identity';$id}
function Bytes($path){[IO.File]::ReadAllText($path)}
function Decode($record){Assert ($record.encoding-eq'windows-utf16-le') 'Unexpected native path encoding';$s='';for($i=0;$i-lt$record.units_hex.Length;$i+=4){$s+=[char][Convert]::ToUInt16($record.units_hex.Substring($i,4),16)};$s}
try{
 Assert ($env:USERNAME-eq'OttrinMoveUIQA') 'Wrong actor';$id=[Security.Principal.WindowsIdentity]::GetCurrent();Assert (-not([Security.Principal.WindowsPrincipal]::new($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Elevated actor'
 Assert (-not(Test-Path $install)) 'Unexpected installation';Assert ((Get-Volume -DriveLetter R).FileSystemLabel-eq'OTTRIN_MOVE_QA') 'Wrong fixture volume'
 $fixture="$env:USERPROFILE\MoveQA8";Assert (-not(Test-Path $fixture)) 'Existing source fixture';Assert (-not(Test-Path 'R:\MoveQA8')) 'Existing destination fixture';New-Item -ItemType Directory $fixture,'R:\MoveQA8'|Out-Null
 $c=Get-Content "$b\config-template.json" -Raw|ConvertFrom-Json;$c.theme='Dark';$c.default_view_mode='Miller';$c.window_size=@(1000,700);$c.restore_session=$false;$c.search_setup_completed=$true;$c.graphics_compatibility_notice_seen=$true;$c.bookmarks=@();$c.last_session_dirs=@();$c.search.include_roots=@($fixture);$c.search.exclude_roots=@();$c.search.content_indexing_enabled=$false
 $null=[IO.Directory]::CreateDirectory((Split-Path $pref));[IO.File]::WriteAllText($pref,($c|ConvertTo-Json -Depth 50),[Text.UTF8Encoding]::new($false));$sourceConfig=(Get-FileHash $pref).Hash
 Phase 'install';$installed=$true;Run "$b\current.exe" '/S';$manifest=Get-Content "$b\payload.json" -Raw|ConvertFrom-Json;foreach($e in $manifest.PSObject.Properties){Assert ((Get-FileHash (Join-Path $install $e.Name)).Hash.ToLowerInvariant()-ceq$e.Value) "Payload mismatch $($e.Name)"}
 $env:OTTRIN_DEV_HELPER_AUTOBUILD='0';Remove-Item Env:\OTTRIN_RENDERER -ErrorAction SilentlyContinue
 $volumes=if($CrossOnly){@('cross')}else{@('same','cross')};foreach($volume in $volumes){foreach($choice in @('unoccupied','replace','rename','skip','cancel')){
  $name="$volume-$choice";Phase ('prepare-'+$name);$srcDir="$fixture\$name\s";$dest=if($volume-eq'same'){"$fixture\$name\d"}else{"R:\MoveQA8\$name\d"};New-Item -ItemType Directory $srcDir,$dest|Out-Null
  $source="$srcDir\payload.txt";$target="$dest\payload.txt";$new="source:$name";$old="destination:$name";[IO.File]::WriteAllText($source,$new);$sourceId=FileId $source;$oldId=$null
  if($choice-ne'unoccupied'){[IO.File]::WriteAllText($target,$old);$oldId=FileId $target}
  $gui=Start-Process $app -ArgumentList ('--select "'+$source+'"') -PassThru -RedirectStandardOutput "$r\$name.stdout" -RedirectStandardError "$r\$name.stderr";$null=$gui.Handle;$active+=,$gui;$w=WaitWindow '^Ottrin$';Start-Sleep -Seconds 8;Capture $w ($name+'-root')
  Key $w '^x';Key $w '^l';Key $w '^a';Set-Clipboard -Value $dest;Key $w '^v';Key $w '{ENTER}';Start-Sleep -Milliseconds 800;Key $w '^v';Start-Sleep -Milliseconds 800;Capture $w ($name+'-before-choice')
  if($choice-ne'unoccupied'){
   Assert ((Test-Path $source)-and(Bytes $source)-ceq$new-and(Bytes $target)-ceq$old) 'Conflict mutated files before choice'
   if($choice-eq'cancel'){Key $w '{ESC}'}else{$tabs=@{replace=1;rename=2;skip=3}[$choice];for($k=0;$k-lt$tabs;$k++){Key $w '{TAB}'};Capture $w ($name+'-choice-focus');Key $w '{ENTER}'}
  }
  Phase ('verify-'+$name);$deadline=[DateTime]::UtcNow.AddSeconds(15)
  if($choice-in@('unoccupied','replace','rename')){
   $ready=$false;$lastError=$null
   do{try{$ready=-not(Test-Path $source);if($ready){if($choice-eq'rename'){$found=@(Get-ChildItem $dest -File|Where-Object { $_.Name -ne 'payload.txt' });$ready=$found.Count-eq1-and(Bytes $found[0].FullName)-ceq$new}else{$ready=(Test-Path $target)-and(Bytes $target)-ceq$new}}}catch{$lastError=$_.Exception.Message;$ready=$false};if($ready){break};Start-Sleep -Milliseconds 100}while([DateTime]::UtcNow-lt$deadline)
   Assert $ready "Move publication did not settle: $name : $lastError";Start-Sleep -Seconds 1
  }
  else{Start-Sleep -Seconds 1;Assert ((Bytes $source)-ceq$new-and(FileId $source)-ceq$sourceId) 'Skip/Cancel altered source'}
  $renamed=$null;$retained=$null
  if($choice-eq'rename'){$copies=@(Get-ChildItem $dest -File|Where-Object { $_.Name -ne 'payload.txt' });Assert ($copies.Count-eq1-and(Bytes $copies[0].FullName)-ceq$new-and(Bytes $target)-ceq$old-and(FileId $target)-ceq$oldId) 'Keep Both payload/identity mismatch';$renamed=$copies[0].Name}
  elseif($choice-in@('unoccupied','replace')){Assert ((Bytes $target)-ceq$new) 'Published payload differs';if($volume-eq'same'){Assert ((FileId $target)-ceq$sourceId) 'Same-volume move changed source identity'}}
  else{Assert ((Bytes $target)-ceq$old-and(FileId $target)-ceq$oldId) 'Skip/Cancel altered destination'}
  if($choice-eq'replace'){
   $central="$env:LOCALAPPDATA\Ottrin\State\moves";$matches=@();foreach($j in Get-ChildItem $central -File|Where-Object Extension -in @(".journal",".cross-journal")){try{$record=Get-Content $j.FullName -Raw|ConvertFrom-Json;if($record.retained_displaced){$recordSource=Decode $record.source;if($recordSource.StartsWith('\\?\')){$recordSource=$recordSource.Substring(4)};$path=Decode $record.retained_displaced;if($recordSource-ceq$source-and(Test-Path $path)-and(Bytes $path)-ceq$old){$matches+=,@{path=$path;journal=$j.FullName;record=$record}}}}catch{throw}}
   Assert ($matches.Count-eq1-and(FileId $matches[0].path)-ceq$oldId) 'Displaced destination retention mismatch';$retained=$matches[0];$retained.record|ConvertTo-Json -Depth 20|Set-Content "$r\$name-journal.json"
  }
  Capture $w ($name+'-after');$gui.Refresh();$null=$gui.CloseMainWindow();Assert ($gui.WaitForExit(15000)-and$gui.ExitCode-eq0) 'GUI failed clean exit';$gui=$null
  $result.cases+=@{name=$name;status='pass';source_id=$sourceId;destination_original_id=$oldId;renamed=$renamed;retained=$retained;source_removed=($choice-in@('unoccupied','replace','rename'))};Phase ('accepted-'+$name)
 }}
 $result.installer_sha256=(Get-FileHash "$b\current.exe").Hash.ToLowerInvariant();$result.status='pass';Phase 'accepted'
}catch{$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally{
 $errors=@();foreach($p in $active){try{StopTree $p}catch{$errors+=$_.Exception.Message}}
 try{if($installed-and(Test-Path "$install\Uninstall.exe")){Run "$install\bin\ottrin-indexd.exe" '--stop';Copy-Item "$install\Uninstall.exe" "$r\uninstall.exe";Run "$r\uninstall.exe" "/S _?=$install";Assert (-not(Test-Path $install)) 'Payload remains after normal uninstall'}}catch{$errors+=$_.Exception.Message}
 if($errors.Count){$result.status='fail';$result.cleanup_errors=$errors};$result|ConvertTo-Json -Depth 30|Set-Content "$r\RESULTS.json"
}
if($result.status-ne'pass'){exit 1}
