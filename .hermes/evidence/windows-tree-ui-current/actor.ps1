param([ValidateSet('All','directory','batch')][string]$Kind='All', [ValidatePattern('^TreeQA[0-9]*$')][string]$RunName='TreeQA', [ValidatePattern('^(same|cross)-(directory|batch)-(unoccupied|replace|rename|skip|cancel)$|^$')][string]$OnlyCase='')
$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'
$b='C:\OttrinTreeUIQA'; $r="$b\results"; $install="$env:LOCALAPPDATA\Programs\Ottrin"; $app="$install\bin\ottrin.exe"; $pref="$env:APPDATA\ottrin\config.json"
$active=@(); $gui=$null; $installed=$false
$result=@{status='running';phase='prepare';cases=@();scope='Exact NSIS ordinary-user GUI directory and mixed-batch Move on owned NTFS volumes'}
function Assert($value,$message) { if (-not $value) { throw $message } }
function StopTree($process) { if ($process -and -not $process.HasExited) { & "$env:WINDIR\System32\taskkill.exe" /PID $process.Id /T /F | Out-Null; Assert ($LASTEXITCODE -eq 0) 'Cannot stop owned process tree'; Assert ($process.WaitForExit(10000)) 'Owned process remains live' } }
function Run($program,$arguments) { $process=Start-Process $program -ArgumentList $arguments -PassThru; $null=$process.Handle; $script:active+=,$process; Assert ($process.WaitForExit(180000)) 'Owned command remains live'; Assert ($process.ExitCode -eq 0) "Owned command failed: $program ($($process.ExitCode))" }
function SaveJson($object,$path) { $object | ConvertTo-Json -Depth 40 | Set-Content -LiteralPath $path -Encoding UTF8 }
function Phase($name) { $result.phase=$name; SaveJson $result "$r\PROGRESS.json" }
. "$b\snapshot.ps1"
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
function Capture($w,$name){$rect=[OttrinPreviewGuiQA+RECT]::new();$origin=[OttrinPreviewGuiQA+POINT]::new();Assert ([OttrinPreviewGuiQA]::GetClientRect($w.Handle,[ref]$rect)) 'Missing client geometry';Assert ([OttrinPreviewGuiQA]::ClientToScreen($w.Handle,[ref]$origin)) 'Missing client origin';$bitmap=[Drawing.Bitmap]::new(($rect.Right-$rect.Left),($rect.Bottom-$rect.Top));$g=[Drawing.Graphics]::FromImage($bitmap);try{$g.CopyFromScreen($origin.X,$origin.Y,0,0,$bitmap.Size);$bitmap.Save("$r\TREE-$name.png",[Drawing.Imaging.ImageFormat]::Png)}finally{$g.Dispose();$bitmap.Dispose()}}
function Focus($w){
 if([OttrinPreviewGuiQA]::GetForegroundWindow()-eq$w.Handle){return}
 $null=[OttrinPreviewGuiQA]::ShowWindow($w.Handle,9);$deadline=[DateTime]::UtcNow.AddSeconds(5)
 do{$null=[OttrinPreviewGuiQA]::SetForegroundWindow($w.Handle);$rect=[OttrinPreviewGuiQA+RECT]::new();Assert ([OttrinPreviewGuiQA]::GetWindowRect($w.Handle,[ref]$rect)) 'Missing focus geometry';$null=[OttrinPreviewGuiQA]::SetCursorPos(($rect.Left+200),($rect.Top+16));[OttrinPreviewGuiQA]::mouse_event(2,0,0,0,[UIntPtr]::Zero);[OttrinPreviewGuiQA]::mouse_event(4,0,0,0,[UIntPtr]::Zero);Start-Sleep -Milliseconds 250;if([OttrinPreviewGuiQA]::GetForegroundWindow()-eq$w.Handle){return}}while([DateTime]::UtcNow-lt$deadline)
 Capture $w 'focus-failure';throw ('Owned GUI not foreground; expected '+$w.Handle+' actual '+[OttrinPreviewGuiQA]::GetForegroundWindow())
}
function Key($w,$key){Focus $w;[Windows.Forms.SendKeys]::SendWait($key);Start-Sleep -Milliseconds 400}


function SeedTree($path,$marker,[bool]$old) {
    New-Item -ItemType Directory -Path $path,"$path\nested folder",$(if ($old) {"$path\old-empty"} else {"$path\empty"}) | Out-Null
    $rootName=if ($old) {'legacy.txt'} else {'root.txt'}
    [IO.File]::WriteAllText("$path\$rootName",$marker+':root')
    [IO.File]::WriteAllText((Join-Path "$path\nested folder" ('note'+[char]0xE9+'.txt')),$marker+':nested')
}
try {
    Assert ($env:USERNAME -eq 'OttrinTreeUIQA') 'Wrong account'
    $identity=[Security.Principal.WindowsIdentity]::GetCurrent()
    Assert (-not ([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Elevated actor'
    Assert (-not (Test-Path $install)) 'Unexpected installation'
    Assert ((Get-Volume -DriveLetter R).FileSystemLabel -eq 'OTTRIN_TREE_QA') 'Wrong fixture volume'
    $fixture="$env:USERPROFILE\$RunName"; $other="R:\$RunName"
    Assert (-not (Test-Path $fixture) -and -not (Test-Path $other)) 'Fixture namespace already exists'
    New-Item -ItemType Directory -Path $fixture,$other | Out-Null
    $config=Get-Content "$b\config-template.json" -Raw -Encoding UTF8 | ConvertFrom-Json
    $config.theme='Dark'; $config.default_view_mode='Miller'; $config.window_size=@(1000,700); $config.restore_session=$false; $config.search_setup_completed=$true; $config.graphics_compatibility_notice_seen=$true; $config.bookmarks=@(); $config.last_session_dirs=@(); $config.search.include_roots=@($fixture); $config.search.exclude_roots=@(); $config.search.content_indexing_enabled=$false
    $null=[IO.Directory]::CreateDirectory((Split-Path $pref)); [IO.File]::WriteAllText($pref,($config | ConvertTo-Json -Depth 40),[Text.UTF8Encoding]::new($false))
    Phase 'install'; $installed=$true; Run "$b\current.exe" '/S'
    $manifest=Get-Content "$b\payload.json" -Raw | ConvertFrom-Json
    foreach ($entry in $manifest.PSObject.Properties) { Assert ((Get-FileHash -LiteralPath (Join-Path $install $entry.Name)).Hash.ToLowerInvariant() -ceq $entry.Value) "Payload differs: $($entry.Name)" }
    $env:OTTRIN_DEV_HELPER_AUTOBUILD='0'; Remove-Item Env:\OTTRIN_RENDERER -ErrorAction SilentlyContinue
    $kinds=if ($Kind -eq 'All') {@('directory','batch')} else {@($Kind)}
    foreach ($volume in @('same','cross')) { foreach ($mode in $kinds) { foreach ($choice in @('unoccupied','replace','rename','skip','cancel')) {
        $name="$volume-$mode-$choice"; if ($OnlyCase -and $name -ne $OnlyCase) { continue }; Phase ('prepare-'+$name)
        $sourceParent="$fixture\$name\s"; $destination=if ($volume -eq 'same') {"$fixture\$name\d"} else {"$other\$name\d"}
        New-Item -ItemType Directory -Path $sourceParent,$destination | Out-Null
        $folder='bundle '+[char]0x3A9
        $names=if ($mode -eq 'batch') {@('alpha.txt','beta.txt',$folder)} else {@($folder)}
        $entries=@()
        foreach ($leaf in $names) {
            $source=Join-Path $sourceParent $leaf; $target=Join-Path $destination $leaf
            $isFolder=$leaf -ceq $folder
            if ($isFolder) { SeedTree $source ('source:'+$name+':folder') $false } else { [IO.File]::WriteAllText($source,('source:'+$name+':'+$leaf)) }
            $before=@(Snapshot $source); if ($isFolder) { Assert ($before.Count -eq 5) 'Fixture tree enumeration incomplete' }
            $conflict=$choice -ne 'unoccupied' -and $leaf -ne 'beta.txt'
            $destinationBefore=@()
            if ($conflict) { if ($isFolder) { SeedTree $target ('destination:'+$name+':folder') $true } else { [IO.File]::WriteAllText($target,('destination:'+$name+':'+$leaf)) }; $destinationBefore=@(Snapshot $target) }
            $moved=$choice -ne 'cancel' -and -not ($choice -eq 'skip' -and $conflict)
            $published=if ($moved -and -not ($choice -eq 'rename' -and $conflict)) {$target} else {$null}
            $entries+=@{name=$leaf;source=$source;original_target=$target;published=$published;source_before=$before;destination_before=$destinationBefore;moved=$moved;conflicted=$conflict;retained=$null}
        }
        $case=@{name=$name;volume=$volume;kind=$mode;choice=$choice;source_parent=$sourceParent;destination=$destination;entries=$entries}; SaveJson $case "$r\$name-input.json"
        $selected=$sourceParent
        $gui=Start-Process $app -ArgumentList ('"'+$selected+'"') -PassThru -RedirectStandardOutput "$r\$name.stdout" -RedirectStandardError "$r\$name.stderr"; $null=$gui.Handle; $active+=,$gui
        $window=WaitWindow '^Ottrin$'; Start-Sleep -Seconds 8; $result.dpi=[OttrinPreviewGuiQA]::GetDpiForWindow($window.Handle); Capture $window ($name+'-root')
        Key $window '^a'
        Key $window '^x'; Key $window '^l'; Key $window '^a'; Set-Clipboard -Value $destination; Key $window '^v'; Key $window '{ENTER}'; Start-Sleep -Milliseconds 800; Key $window '^v'; Start-Sleep -Milliseconds 800; Capture $window ($name+'-before-choice')
        if ($choice -ne 'unoccupied') {
            foreach ($entry in $entries) { Assert (EqualSnapshot @(Snapshot $entry.source) @($entry.source_before) $true) 'Conflict dialog changed source before choice'; if ($entry.conflicted) { Assert (EqualSnapshot @(Snapshot $entry.original_target) @($entry.destination_before) $true) 'Conflict dialog changed destination before choice' } }
            if ($choice -eq 'cancel') { Key $window '{ESC}' } else { $tabs=@{replace=1;rename=2;skip=3}[$choice]; for ($i=0;$i -lt $tabs;$i++) { Key $window '{TAB}' }; Capture $window ($name+'-choice-focus'); Key $window '{ENTER}' }
        }
        Phase ('verify-'+$name); $deadline=[DateTime]::UtcNow.AddSeconds(30); $verified=$null; $lastError=$null
        do { try { $verified=VerifyCase $case; break } catch { $lastError=$_.Exception.Message; Start-Sleep -Milliseconds 200 } } while ([DateTime]::UtcNow -lt $deadline)
        if ($null -eq $verified) { Capture $window ($name+'-failure') }; Assert ($null -ne $verified) "Move did not settle: $name : $lastError"
        Capture $window ($name+'-after'); $gui.Refresh(); $null=$gui.CloseMainWindow(); Assert ($gui.WaitForExit(15000) -and $gui.ExitCode -eq 0) 'GUI did not close normally'; $gui=$null
        $case.verification=$verified; $result.cases+=$case; Phase ('accepted-'+$name)
    } } }
    $result.installer_sha256=(Get-FileHash "$b\current.exe").Hash.ToLowerInvariant(); $result.status='pass'; Phase 'accepted'
} catch { $result.status='fail'; $result.error=$_.Exception.Message; $result.stack=$_.ScriptStackTrace }
finally {
    $errors=@(); foreach ($process in $active) { try { StopTree $process } catch { $errors+=$_.Exception.Message } }
    try { if ($installed -and (Test-Path "$install\Uninstall.exe")) { Run "$install\bin\ottrin-indexd.exe" '--stop'; Copy-Item "$install\Uninstall.exe" "$r\uninstall.exe" -Force; Run "$r\uninstall.exe" "/S _?=$install"; Assert (-not (Test-Path $install)) 'Uninstall left payload' } } catch { $errors+=$_.Exception.Message }
    if ($errors.Count) { $result.status='fail'; $result.cleanup_errors=$errors }
    SaveJson $result "$r\RESULTS.json"
}
if ($result.status -ne 'pass') { exit 1 }
