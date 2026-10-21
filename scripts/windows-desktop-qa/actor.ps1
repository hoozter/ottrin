$ErrorActionPreference='Stop'
$root=$PSScriptRoot
trap { $_.ToString()|Set-Content -Encoding UTF8 "$root\results\actor-error.txt";exit 1 }
$owner=Get-Content "$root\actor-config.json" -Raw|ConvertFrom-Json
$identity=[Security.Principal.WindowsIdentity]::GetCurrent()
if($identity.User.Value-ne$owner.sid){throw 'Wrong desktop account'}
if(([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Desktop actor must remain ordinary'}
$env:TEMP="$root\actor-temp";$env:TMP=$env:TEMP
. "$root\interop.ps1"
$null=[CopyActivityDesktop]::SetProcessDPIAware()
if([Diagnostics.Process]::GetCurrentProcess().SessionId-ne[CopyActivityDesktop]::WTSGetActiveConsoleSessionId()){throw 'Not the owned active console'}
$c=Get-Content "$root\command.json" -Raw|ConvertFrom-Json
if($c.case-ne'ordinary'){throw 'Unknown disposable case'}
$caseRoot="$root\cases\ordinary";$fixture="$caseRoot\fixture";$app="$root\payload\Ottrin-0.1.0-windows-x64\bin\ottrin.exe"
$env:LOCALAPPDATA="$caseRoot\profile";$env:APPDATA="$caseRoot\roaming";$env:USERPROFILE="$caseRoot\userprofile";$env:HOME=$env:USERPROFILE
function FixturePath([string]$relative){
 if([IO.Path]::IsPathRooted($relative)){throw 'Fixture path must be relative'}
 $path=[IO.Path]::GetFullPath((Join-Path $fixture $relative))
 if($path-ne$fixture-and-not$path.StartsWith($fixture+'\',[StringComparison]::OrdinalIgnoreCase)){throw 'Fixture path escaped'}
 return $path
}
function WriteFixtureConfig {
 $config=Get-Content "$root\config-template.json" -Raw|ConvertFrom-Json
 $config.theme='Dark';$config.default_view_mode='List';$config.bookmarks=@();$config.last_session_dirs=@();$config.restore_session=$false
 $config.search.include_roots=@($fixture);$config.search.exclude_roots=@();$config.search.use_system_locate=$false;$config.search.manage_locate_db=$false;$config.search.refresh_interval_hours=0;$config.search.content_indexing_enabled=$false
 [IO.File]::WriteAllText("$env:APPDATA\ottrin\config.json",($config|ConvertTo-Json -Depth 40),[Text.UTF8Encoding]::new($false))
}
$processes=@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-eq$app})
if($processes.Count-gt1){throw 'Multiple owned GUI processes'}
$verification=$null
if($c.op-eq'prepare'){
 if($processes.Count-or(Test-Path $caseRoot)){throw 'Owned case already exists'}
 New-Item -ItemType Directory "$fixture\source","$fixture\destination","$fixture\moved",$env:USERPROFILE,"$env:APPDATA\ottrin",$env:LOCALAPPDATA|Out-Null
 $utf8=[Text.UTF8Encoding]::new($false)
 [IO.File]::WriteAllText("$fixture\source\note.txt","source original`n",$utf8)
 [IO.File]::WriteAllText("$fixture\source\other.txt","other original`n",$utf8)
 [IO.File]::WriteAllText("$fixture\source\contact.vcf","BEGIN:VCARD`r`nVERSION:3.0`r`nFN:Ottrin QA`r`nEND:VCARD`r`n",$utf8)
 [IO.File]::WriteAllText("$fixture\destination\note.txt","destination original`n",$utf8)
 WriteFixtureConfig
 $baseline=@{};foreach($f in Get-ChildItem "$fixture\source","$fixture\destination" -File){$baseline[$f.FullName]=(Get-FileHash $f.FullName).Hash};$baseline|ConvertTo-Json|Set-Content "$caseRoot\fixture-baseline.json"
}elseif($c.op-eq'configure'){
 if($processes.Count-or-not(Test-Path "$caseRoot\fixture-baseline.json")){throw 'Case must be prepared and closed'}
 WriteFixtureConfig
}elseif($c.op-eq'launch'){
 if($processes.Count){throw 'Owned GUI already running'}
 $arguments=@("$fixture\source");if($c.renderer-eq'opengl'){$arguments+=@('--renderer','opengl')}elseif($c.renderer-and$c.renderer-ne'auto'){throw 'Unknown renderer'}
 $p=Start-Process $app -ArgumentList $arguments -WorkingDirectory (Split-Path $app) -PassThru -RedirectStandardOutput "$root\results\app.stdout" -RedirectStandardError "$root\results\app.stderr"
 $deadline=[DateTime]::UtcNow.AddSeconds(35)
 $ready=$false
 do{
  $p.Refresh();if($p.HasExited){throw "App exited: $($p.ExitCode)"}
  foreach($h in [CopyActivityDesktop]::Windows($p.Id)){$rect=New-Object CopyActivityDesktop+RECT;$null=[CopyActivityDesktop]::GetWindowRect($h,[ref]$rect);if(($rect.Right-$rect.Left)-gt300-and($rect.Bottom-$rect.Top)-gt200){$ready=$true}}
  if($ready){break};Start-Sleep -Milliseconds 100
 }while([DateTime]::UtcNow-lt$deadline)
 if(-not$ready){throw 'Owned usable window not ready'}
 Start-Sleep -Milliseconds 500
}elseif($c.op-eq'close'){
 foreach($owned in $processes){$p=Get-Process -Id $owned.ProcessId;if(-not$p.CloseMainWindow()){throw 'Main window refused close'};if(-not$p.WaitForExit(10000)){throw 'App remains live'}}
 foreach($daemon in @(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-eq"$root\payload\Ottrin-0.1.0-windows-x64\bin\ottrin-indexd.exe"})){Stop-Process -Id $daemon.ProcessId -ErrorAction Stop}
 if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like"$root\payload\*"}).Count){throw 'Owned payload processes remain'}
}elseif($c.op-eq'native-test'){
 if($processes.Count-or$c.tag-notmatch'^[a-z0-9-]+$'){throw 'Native test must own an isolated closed case'}
 if($c.sha256-notmatch'^[a-f0-9]{64}$'-or(Get-FileHash "$root\native-tests.exe").Hash.ToLower()-ne$c.sha256){throw 'Native regression executable identity mismatch'}
 $filter='windows_native_completed_overwrite_';if($c.filter){$filter=$c.filter}
 if($filter-notin@('windows_native_completed_overwrite_','move_transaction::windows_source_tests::')){throw 'Unexpected native regression scope'}
 $p=Start-Process "$root\native-tests.exe" -ArgumentList @($filter,'--nocapture','--test-threads=1') -PassThru -RedirectStandardOutput "$root\results\$($c.tag).stdout" -RedirectStandardError "$root\results\$($c.tag).stderr"
 $null=$p.Handle
 try{if(-not$p.WaitForExit(90000)){throw 'Native regression timed out'};$exit=$p.ExitCode}finally{if(-not$p.HasExited){Stop-Process -Id $p.Id -ErrorAction Stop;$null=$p.WaitForExit(10000)}}
 if($exit-ne$c.expected_exit){throw "Unexpected native regression exit $exit"}
 $verification=@{label=$c.tag;native_exit=$exit;expected_exit=$c.expected_exit}
}elseif($c.op-eq'verify-retained'){
 function DecodePath($record){if($record.encoding-ne'windows-utf16-le'){throw 'Unexpected journal path encoding'};$s='';for($i=0;$i-lt$record.units_hex.Length;$i+=4){$s+=[char][Convert]::ToUInt16($record.units_hex.Substring($i,4),16)};return $s}
 $matches=@()
 foreach($j in Get-ChildItem "$env:LOCALAPPDATA\Ottrin\State\moves" -File|Where-Object Extension -in @('.journal','.cross-journal')){
  $record=Get-Content $j.FullName -Raw|ConvertFrom-Json
  if($record.retained_displaced){$path=DecodePath $record.retained_displaced;if([IO.File]::ReadAllText($path)-ceq"destination original`n"){
   if($record.phase-ne'published'){throw 'Retained Move was not published'}
   $stage=DecodePath $record.transaction_directory;$local=@(Get-ChildItem $stage -File -Force|Where-Object Name -like '*journal*')
   if($local.Count-ne1-or[IO.File]::ReadAllText($local[0].FullName).Trim()-cne[IO.File]::ReadAllText($j.FullName).Trim()){throw 'Retained journal pair differs'}
   $matches+=@{central_sha256=(Get-FileHash $j.FullName).Hash;local_sha256=(Get-FileHash $local[0].FullName).Hash;displaced_sha256=(Get-FileHash $path).Hash}
  }}
 }
 if($matches.Count-ne1){throw 'Expected one exact retained displaced destination'}
 $verification=@{status='pass';label='Replace preserves displaced bytes with paired published journals';matches=$matches}
}elseif($c.op-eq'verify'){
 foreach($item in $c.files.PSObject.Properties){$path=FixturePath $item.Name;if(-not(Test-Path -LiteralPath $path -PathType Leaf)-or[IO.File]::ReadAllText($path)-cne[string]$item.Value){throw ('Fixture mismatch: '+$item.Name)}}
 foreach($relative in $c.absent){if(Test-Path -LiteralPath (FixturePath $relative)){throw ('Fixture unexpectedly exists: '+$relative)}}
 $verification=@{status='pass';files=$c.files;absent=$c.absent;label=$c.label}
}elseif($c.op-eq'onboard'){
 [uint32]$foregroundPid=0;$null=[CopyActivityDesktop]::GetWindowThreadProcessId([CopyActivityDesktop]::GetForegroundWindow(),[ref]$foregroundPid)
 $foreground=Get-CimInstance Win32_Process|Where-Object{$_.ProcessId-eq$foregroundPid}
 if($foregroundPid-ne$c.foreground_pid-or$foreground.ExecutablePath-ine"$env:windir\system32\wwahost.exe"-or(Invoke-CimMethod $foreground -MethodName GetOwner).User-ne$owner.account){throw 'Owned Windows onboarding surface changed'}
 $screen=[Windows.Forms.SystemInformation]::VirtualScreen;if(-not$screen.Contains([int]$c.x,[int]$c.y)){throw 'Onboarding point outside console'}
 $null=[CopyActivityDesktop]::SetCursorPos([int]$c.x,[int]$c.y);[CopyActivityDesktop]::mouse_event(2,0,0,0,[UIntPtr]::Zero);[CopyActivityDesktop]::mouse_event(4,0,0,0,[UIntPtr]::Zero);Start-Sleep -Milliseconds 700
}elseif($c.op-in@('focus','click','key','type','nav')){
 if($processes.Count-ne1){throw 'Owned GUI missing'}
 $p=Get-Process -Id $processes[0].ProcessId;$p.Refresh()
 [uint32]$foregroundPid=0;$window=[CopyActivityDesktop]::GetForegroundWindow();$null=[CopyActivityDesktop]::GetWindowThreadProcessId($window,[ref]$foregroundPid)
 if($c.window){$window=[IntPtr][long]$c.window}elseif($foregroundPid-ne$p.Id){$window=$p.MainWindowHandle}
 [uint32]$windowPid=0;$null=[CopyActivityDesktop]::GetWindowThreadProcessId($window,[ref]$windowPid);if($windowPid-ne$p.Id){throw 'Target window is not owned'}
 if($c.op-in@('click','focus')){
  $point=New-Object CopyActivityDesktop+POINT;$point.X=[int]$c.x;$point.Y=[int]$c.y
  [uint32]$pointPid=0;$null=[CopyActivityDesktop]::GetWindowThreadProcessId([CopyActivityDesktop]::WindowFromPoint($point),[ref]$pointPid)
  if($pointPid-ne$p.Id){throw 'Visible input point is not owned'}
  $null=[CopyActivityDesktop]::SetCursorPos($point.X,$point.Y);[CopyActivityDesktop]::mouse_event(2,0,0,0,[UIntPtr]::Zero);[CopyActivityDesktop]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
 }else{
  $null=[CopyActivityDesktop]::SetForegroundWindow($window);Start-Sleep -Milliseconds 100
  $null=[CopyActivityDesktop]::GetWindowThreadProcessId([CopyActivityDesktop]::GetForegroundWindow(),[ref]$foregroundPid);if($foregroundPid-ne$p.Id){throw 'Owned app did not receive focus'}
  if($c.op-eq'key'){
   if($c.key-notin@('{ESC}','^l','^c','^x','^v','^z','^a','{ENTER}','{F2}','{LEFT}','{RIGHT}',' ','{TAB}','+{TAB}')){throw 'Key outside QA input set'}
   [Windows.Forms.SendKeys]::SendWait($c.key)
  }elseif($c.op-eq'nav'){
   [Windows.Forms.SendKeys]::SendWait('^l');Start-Sleep -Milliseconds 100;[Windows.Forms.Clipboard]::SetText((FixturePath $c.path));[Windows.Forms.SendKeys]::SendWait('^v');[Windows.Forms.SendKeys]::SendWait('{ENTER}')
  }else{[Windows.Forms.Clipboard]::SetText([string]$c.text);[Windows.Forms.SendKeys]::SendWait('^v')}
 }
 Start-Sleep -Milliseconds 300
}elseif($c.op-ne'observe'){throw 'Unknown desktop operation'}
$processes=@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-eq$app})
$windows=@();foreach($process in $processes){foreach($h in [CopyActivityDesktop]::Windows($process.ProcessId)){$rect=New-Object CopyActivityDesktop+RECT;$null=[CopyActivityDesktop]::GetWindowRect($h,[ref]$rect);$title=New-Object Text.StringBuilder 512;$null=[CopyActivityDesktop]::GetWindowText($h,$title,512);$windows+=@{handle=$h.ToInt64();title=$title.ToString();left=$rect.Left;top=$rect.Top;right=$rect.Right;bottom=$rect.Bottom}}}
$screen=[Windows.Forms.SystemInformation]::VirtualScreen;$bitmap=New-Object Drawing.Bitmap($screen.Width,$screen.Height);$g=[Drawing.Graphics]::FromImage($bitmap)
try{$g.CopyFromScreen($screen.Left,$screen.Top,0,0,$bitmap.Size);$bitmap.Save("$root\results\desktop.png",[Drawing.Imaging.ImageFormat]::Png)}finally{$g.Dispose();$bitmap.Dispose()}
[uint32]$foregroundPid=0;$null=[CopyActivityDesktop]::GetWindowThreadProcessId([CopyActivityDesktop]::GetForegroundWindow(),[ref]$foregroundPid)
@{id=$c.id;op=$c.op;case=$c.case;ordinary_user=$true;windows=$windows;foreground_pid=$foregroundPid;verification=$verification;screen_width=$screen.Width;screen_height=$screen.Height}|ConvertTo-Json -Depth 12|Set-Content -Encoding UTF8 "$root\results\desktop.json"
