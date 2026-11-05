param([Parameter(Mandatory=$true)][ValidateSet('Prepare','Install','Actor','Login','Finalize','Updated','AfterRemoval')][string]$Mode,[string]$Disc,[string]$Root='C:\OttrinOwnedMsixRuntime')
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
function Save($Name,$Value){
 $path=Join-Path $Root ('results\'+$Name+'.json');$temporary=$path+'.'+[Guid]::NewGuid().ToString('N')+'.tmp'
 try{
  [IO.File]::WriteAllText($temporary,($Value|ConvertTo-Json -Depth 10),[Text.UTF8Encoding]::new($false))
  if([IO.File]::Exists($path)){[IO.File]::Replace($temporary,$path,[NullString]::Value)}else{[IO.File]::Move($temporary,$path)}
 }finally{if([IO.File]::Exists($temporary)){[IO.File]::Delete($temporary)}}
}
function Read-State([string]$Path){
 # Atomic File.Replace briefly takes an exclusive destination handle on Windows.
 # Retry only sharing/lock violations; malformed state and other IO failures fail.
 $readDeadline=[DateTime]::UtcNow.AddSeconds(2)
 while($true){
  try{
   $stream=[IO.File]::Open($Path,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite-bor[IO.FileShare]::Delete))
   $reader=[IO.StreamReader]::new($stream)
   try{return ($reader.ReadToEnd()|ConvertFrom-Json)}finally{$reader.Dispose()}
  }catch{
   $failure=$_.Exception.GetBaseException();$code=$failure.HResult-band0xffff
   if($failure-isnot[IO.IOException]-or$code-notin@(32,33)-or[DateTime]::UtcNow-ge$readDeadline){throw}
   Start-Sleep -Milliseconds 10
  }
 }
}
function Test-OwnedStateReader{
 Add-Type -TypeDefinition @'
using System;using System.Threading;
public static class OwnedStateLockQA {
 public static Thread ReleaseAfter(IDisposable handle,int milliseconds){var thread=new Thread(()=>{Thread.Sleep(milliseconds);handle.Dispose();});thread.Start();return thread;}
}
'@
 $dir=Join-Path $Root 'state-reader-smoke';$null=New-Item -ItemType Directory $dir
 $file=Join-Path $dir 'state.json';$lock=$null;$release=$null
 try{
  [IO.File]::WriteAllText($file,'{"version":17}',[Text.UTF8Encoding]::new($false))
  $lock=[IO.File]::Open($file,'Open','ReadWrite','None');$release=[OwnedStateLockQA]::ReleaseAfter($lock,150)
  $clock=[Diagnostics.Stopwatch]::StartNew();$value=Read-State $file;$release.Join();$release=$null;$lock=$null
  if($value.version-ne17-or$clock.Elapsed.TotalMilliseconds-lt50){throw 'State reader did not survive the controlled exclusive lock'}
  $transientMilliseconds=$clock.Elapsed.TotalMilliseconds
  [IO.File]::WriteAllText($file,'{broken',[Text.UTF8Encoding]::new($false));$malformedRejected=$false
  try{$null=Read-State $file}catch{$malformedRejected=$true}
  if(-not$malformedRejected){throw 'State reader hid malformed JSON'}
  $missingRejected=$false;try{$null=Read-State (Join-Path $dir 'missing.json')}catch{$missingRejected=$true}
  if(-not$missingRejected){throw 'State reader hid a missing file'}
  [IO.File]::WriteAllText($file,'{"version":17}',[Text.UTF8Encoding]::new($false))
  $lock=[IO.File]::Open($file,'Open','ReadWrite','None');$clock.Restart();$lockedRejected=$false
  try{$null=Read-State $file}catch{$lockedRejected=$true}
  if(-not$lockedRejected-or$clock.Elapsed.TotalSeconds-gt3){throw 'State reader did not fail within its bounded lock deadline'}
  $lock.Dispose();$lock=$null
  $persistentSeconds=$clock.Elapsed.TotalSeconds
 }finally{
  if($release){$release.Join()};if($lock){$lock.Dispose()}
  Remove-Item $dir -Recurse -Force
 }
 Save 'STATE_READER_SMOKE' @{status='pass native sharing retry, bounded persistent lock, malformed JSON and missing file refusal';transient_milliseconds=$transientMilliseconds;persistent_lock_seconds=$persistentSeconds;private_fixture_removed=(-not(Test-Path $dir))}
}
if($Mode-eq'Prepare'){
 if(Test-Path $Root){throw 'Existing runtime fixture retained'}
 $packageInput=Get-Content (Join-Path $Disc 'runtime-input.json') -Raw|ConvertFrom-Json
 if((Get-FileHash (Join-Path $Disc 'Ottrin.msix')).Hash.ToLowerInvariant()-ne$packageInput.sha256){throw 'Runtime package input mismatch'}
 if($packageInput.startup_enabled-isnot[bool]){throw 'Explicit expected startup choice required'}
 if($packageInput.previous-and(Get-FileHash (Join-Path $Disc 'Previous.msix')).Hash.ToLowerInvariant()-ne$packageInput.previous.sha256){throw 'Previous runtime package input mismatch'}
 New-Item -ItemType Directory $Root,(Join-Path $Root 'results')|Out-Null
 Test-OwnedStateReader
 $nonce=[Guid]::NewGuid().ToString('N');$name='OTMSIX'+$nonce.Substring(0,12);$secret=[Guid]::NewGuid().ToString('N')+'aA1!';$password=ConvertTo-SecureString $secret -AsPlainText -Force
 try {
  $user=New-LocalUser -Name $name -Password $password -Description ('Owned MSIX QA '+$nonce) -PasswordNeverExpires
  foreach($group in @('S-1-5-32-545','S-1-5-32-544')){Add-LocalGroupMember -Group (Get-LocalGroup -SID $group) -Member $user}
  $acl=Get-Acl $Root;$acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($user.SID,'ReadAndExecute','ContainerInherit,ObjectInherit','None','Allow'));Set-Acl $Root $acl
  $acl=Get-Acl (Join-Path $Root 'results');$acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($user.SID,'Modify','ContainerInherit,ObjectInherit','None','Allow'));Set-Acl (Join-Path $Root 'results') $acl
  Copy-Item (Join-Path $Disc 'Ottrin.msix'),(Join-Path $Disc 'runtime.ps1'),(Join-Path $Disc 'native-controls.ps1') $Root
  if($packageInput.previous){Copy-Item (Join-Path $Disc 'Previous.msix') $Root}
  $owner=@{nonce=$nonce;account=$name;sid=$user.SID.Value;package=$packageInput;installer_task=('OttrinMsixInstall-'+$nonce);actor_task=('OttrinMsixActor-'+$nonce);login_task=('OttrinMsixLogin-'+$nonce);finalizer_task=('OttrinMsixFinalize-'+$nonce);updated_task=('OttrinMsixUpdated-'+$nonce);removed_task=('OttrinMsixRemoved-'+$nonce);scope='Disposable clone only; administrative unsigned deployment, then a filtered ordinary runtime token; no signing-policy changes'}
  $owner|ConvertTo-Json -Depth 6|Set-Content -Encoding UTF8 (Join-Path $Root 'OWNER.json')
  foreach($phase in @('Install','Actor','Login','Finalize','Updated','AfterRemoval')){
   $invoke="& ([ScriptBlock]::Create([IO.File]::ReadAllText('$Root\runtime.ps1'))) -Mode $phase -Root '$Root'"
   $action=New-ScheduledTaskAction -Execute "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe" -Argument ('-NoProfile -NonInteractive -WindowStyle Hidden -EncodedCommand '+[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($invoke)))
   $principal=New-ScheduledTaskPrincipal -UserId ($env:COMPUTERNAME+'\'+$name) -LogonType Interactive -RunLevel $(if($phase-in@('Install','Finalize')){'Highest'}else{'Limited'})
   if($phase-eq'Install'){$trigger=New-ScheduledTaskTrigger -AtLogOn -User ($env:COMPUTERNAME+'\'+$name);Register-ScheduledTask -TaskName $owner.installer_task -Action $action -Principal $principal -Trigger $trigger|Out-Null}else{$taskName=switch($phase){'Actor'{$owner.actor_task};'Login'{$owner.login_task};'Finalize'{$owner.finalizer_task};'Updated'{$owner.updated_task};'AfterRemoval'{$owner.removed_task}};Register-ScheduledTask -TaskName $taskName -Action $action -Principal $principal|Out-Null}
  }
  $key='HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon'
  foreach($pair in @(@('AutoAdminLogon','1'),@('DefaultUserName',$name),@('DefaultDomainName',$env:COMPUTERNAME),@('DefaultPassword',$secret))){Set-ItemProperty $key -Name $pair[0] -Value $pair[1]}
  # Replace the stock image's one-use count so the actual reboot can sign in.
  Set-ItemProperty $key -Name AutoLogonCount -Type DWord -Value 3
  @{status='prepared';account=$name;sid=$user.SID.Value;scope=$owner.scope}|ConvertTo-Json -Compress
 }finally{$secret=$null;$password.Dispose()}
 exit 0
}
$owner=Get-Content (Join-Path $Root 'OWNER.json') -Raw|ConvertFrom-Json
$identity=[Security.Principal.WindowsIdentity]::GetCurrent();if($identity.User.Value-ne$owner.sid-or(Get-Process -Id $PID).SessionId-le0){throw 'Wrong owned interactive identity'}
$admin=([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
 Add-Type -TypeDefinition @'
using System;using System.Text;using System.Runtime.InteropServices;
public static class InputDesktopQA {
 [DllImport("user32.dll",SetLastError=true)]static extern IntPtr OpenInputDesktop(uint flags,bool inherit,uint access);
 [DllImport("user32.dll",CharSet=CharSet.Unicode,SetLastError=true)]static extern bool GetUserObjectInformation(IntPtr value,int index,StringBuilder text,uint length,out uint needed);
 [DllImport("user32.dll")]static extern bool CloseDesktop(IntPtr value);
 [DllImport("advapi32.dll",SetLastError=true)]static extern bool OpenProcessToken(IntPtr process,uint access,out IntPtr token);
 [DllImport("advapi32.dll",SetLastError=true)]static extern bool GetTokenInformation(IntPtr token,int kind,out int elevation,int size,out int needed);
 [DllImport("kernel32.dll")]static extern bool CloseHandle(IntPtr handle);
 [DllImport("kernel32.dll")]public static extern uint WTSGetActiveConsoleSessionId();
 [DllImport("user32.dll")]public static extern bool IsIconic(IntPtr handle);
 [DllImport("user32.dll")]public static extern bool ShowWindow(IntPtr handle,int command);
 [DllImport("user32.dll")]public static extern IntPtr GetForegroundWindow();
 [DllImport("dwmapi.dll")]static extern int DwmGetWindowAttribute(IntPtr handle,int attribute,out int value,int size);
 public static int Cloaked(IntPtr handle){int value;int result=DwmGetWindowAttribute(handle,14,out value,4);if(result!=0)throw new Exception("DWM inspection failed: "+result);return value;}
 public static bool Elevated(IntPtr process){IntPtr token;if(!OpenProcessToken(process,8,out token))throw new System.ComponentModel.Win32Exception();try{int value,needed;if(!GetTokenInformation(token,20,out value,4,out needed))throw new System.ComponentModel.Win32Exception();return value!=0;}finally{CloseHandle(token);}}
 public static string Name(){var h=OpenInputDesktop(0,false,1);if(h==IntPtr.Zero)return "unavailable";try{var b=new StringBuilder(256);uint needed;return GetUserObjectInformation(h,2,b,512,out needed)?b.ToString():"unavailable";}finally{CloseDesktop(h);}}
}
'@

 Add-Type -TypeDefinition 'using System; using System.Text; using System.Runtime.InteropServices; public static class PackageIdentityQA { [DllImport("kernel32.dll", CharSet=CharSet.Unicode)] public static extern int GetPackageFullName(IntPtr process, ref uint length, StringBuilder value); [DllImport("user32.dll", SetLastError=true)] public static extern bool PostMessageW(IntPtr window, uint message, IntPtr first, IntPtr second); [DllImport("user32.dll", SetLastError=true)] public static extern bool SetWindowPos(IntPtr window, IntPtr after, int x, int y, int width, int height, uint flags); }'
$ActorStateName=if($Mode-eq'Login'){'LOGIN'}elseif($Mode-eq'Updated'){'UPDATED'}else{'ACTOR'}
function Invoke-OwnCLI([string[]]$arguments,[string]$stdoutName,[string]$stderrName){
 $p=Start-Process $exe -ArgumentList $arguments -PassThru -WindowStyle Hidden -RedirectStandardOutput (Join-Path $Root ('results\'+$stdoutName)) -RedirectStandardError (Join-Path $Root ('results\'+$stderrName));$null=$p.Handle
 $result.phase=$stdoutName;$result.active_cli=@{pid=$p.Id;arguments=$arguments};Save $ActorStateName $result
 try{
  # Start-Process -Wait waits for descendants, including the persistent daemon.
  # Only the CLI must exit; the daemon is verified separately below.
  if(-not$p.WaitForExit(90000)){throw ('Owned CLI timed out: '+$stdoutName)}
  $p.WaitForExit();$code=$p.ExitCode;$result.cli_exits[$stdoutName]=$code;$result.Remove('active_cli');Save $ActorStateName $result
  return [pscustomobject]@{ExitCode=$code}
 }finally{if(-not$p.HasExited){$p.Kill();$null=$p.WaitForExit(5000)};$p.Dispose()}
}
Add-Type -TypeDefinition @'
using System;using System.IO;using System.Runtime.InteropServices;using Microsoft.Win32.SafeHandles;
public static class StoreFileQA {
 [StructLayout(LayoutKind.Sequential)]struct FileId { public ulong Volume,Low,High; }
 [DllImport("kernel32.dll",SetLastError=true)]static extern bool GetFileInformationByHandleEx(SafeFileHandle handle,int kind,out FileId value,uint size);
 public static string Identity(string path){using(var file=File.Open(path,FileMode.Open,FileAccess.Read,FileShare.ReadWrite|FileShare.Delete)){FileId value;if(!GetFileInformationByHandleEx(file.SafeFileHandle,18,out value,24))throw new System.ComponentModel.Win32Exception();return value.Volume.ToString("X16")+":"+value.Low.ToString("X16")+value.High.ToString("X16");}}
}
'@
function Get-OwnStore{
 $paths=@((Join-Path $env:LOCALAPPDATA 'ottrin\search.db'),(Join-Path $env:LOCALAPPDATA ('Packages\'+$package.PackageFamilyName+'\LocalCache\Local\ottrin\search.db')))
 $existing=@($paths|Where-Object {Test-Path $_ -PathType Leaf})
 if($existing.Count-ne1){throw 'Expected exactly one owned ordinary or package-virtualized search store'}
 return @{path=$existing[0];identity=[StoreFileQA]::Identity($existing[0]);bytes=(Get-Item $existing[0]).Length}
}
function Assert-OwnStartup([string]$Prefix){
 $process=Invoke-OwnCLI @('index','service-status') ($Prefix+'service-status.stdout') ($Prefix+'service-status.stderr')
 $expected=if($owner.package.startup_enabled){'Enabled at login'}else{'enabled=false'}
 if($process.ExitCode-ne0-or(Get-Content (Join-Path $Root ('results\'+$Prefix+'service-status.stdout')) -Raw)-notmatch$expected){throw 'Saved startup choice changed; no repair attempted'}
}
function Wait-OwnIndexReady([string]$Prefix){
 $deadline=[DateTime]::UtcNow.AddSeconds(60)
 do{
  $process=Invoke-OwnCLI @('index','status','--json') ($Prefix+'index-status.json') ($Prefix+'index-status.stderr')
  if($process.ExitCode-ne0){throw 'Read-only index diagnostics failed'}
  $diagnostics=Read-State (Join-Path $Root ('results\'+$Prefix+'index-status.json'))
  if($diagnostics.status-eq'ready'-and$diagnostics.watcher_ready-and$diagnostics.watched_roots-eq1){if($diagnostics.indexed_items-ne1){throw 'Fixture scope was not applied: expected exactly one indexed file'};return $diagnostics}
  Start-Sleep -Seconds 1
 }while([DateTime]::UtcNow-lt$deadline)
 throw 'Package daemon did not become ready on its saved fixture root'
}
 Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
 function UiNodes($Handle,$Name){
  $tree=[Windows.Automation.AutomationElement]::FromHandle($Handle)
  return @($tree.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,$Name)))
 }
 function UiNode($Handle,$Name,$Kind){
  $deadline=[DateTime]::UtcNow.AddSeconds(20)
  do{$nodes=@(UiNodes $Handle $Name|Where-Object {$_.Current.ControlType-eq$Kind -and -not$_.Current.IsOffscreen});if($nodes.Count-eq1){return $nodes[0]};Start-Sleep -Milliseconds 100}while([DateTime]::UtcNow-lt$deadline)
  throw ('Missing unique packaged control '+$Name)
 }
 function UiInvoke($Handle,$Name){(UiNode $Handle $Name ([Windows.Automation.ControlType]::Button)).GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke();Start-Sleep -Milliseconds 300}

function Initialize-ShellProbe {
 if('PackagedShellQA'-as[type]){return}
 Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;
[ComImport,Guid("a08ce4d0-fa25-44ab-b57c-c7b1c323e0b9"),InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
interface IOttrinExplorerCommand {
 [PreserveSig]int GetTitle(IntPtr items,out IntPtr title);
 [PreserveSig]int GetIcon(IntPtr items,out IntPtr icon);
 [PreserveSig]int GetToolTip(IntPtr items,out IntPtr tip);
 [PreserveSig]int GetCanonicalName(out Guid name);
 [PreserveSig]int GetState(IntPtr items,[MarshalAs(UnmanagedType.Bool)]bool slow,out uint state);
 [PreserveSig]int Invoke(IntPtr items,IntPtr binding);
 [PreserveSig]int GetFlags(out uint flags);
 [PreserveSig]int EnumSubCommands(out IntPtr commands);
}
public static class PackagedShellQA {
 static readonly Guid Class=new Guid("6aee96d8-191d-476c-a773-b8679c83311c");
 [DllImport("kernel32.dll")]static extern int GetCurrentPackageFullName(ref uint size,IntPtr name);
 [DllImport("shell32.dll",CharSet=CharSet.Unicode)]static extern int SHCreateItemFromParsingName(string name,IntPtr bind,ref Guid iid,out IntPtr item);
 [DllImport("shell32.dll")]static extern int SHCreateShellItemArrayFromShellItem(IntPtr item,ref Guid iid,out IntPtr array);
 static IOttrinExplorerCommand Create(){return (IOttrinExplorerCommand)Activator.CreateInstance(Type.GetTypeFromCLSID(Class,true));}
 [DllImport("shell32.dll",CharSet=CharSet.Unicode,SetLastError=true)]static extern IntPtr CommandLineToArgvW(string command,out int count);
 [DllImport("kernel32.dll")]static extern IntPtr LocalFree(IntPtr value);
 public static string[] Arguments(string command){int count;IntPtr values=CommandLineToArgvW(command,out count);if(values==IntPtr.Zero)throw new System.ComponentModel.Win32Exception();try{var args=new string[count];for(int i=0;i<count;i++)args[i]=Marshal.PtrToStringUni(Marshal.ReadIntPtr(values,i*IntPtr.Size));return args;}finally{LocalFree(values);}}
 public static int HostIdentity(){uint size=0;return GetCurrentPackageFullName(ref size,IntPtr.Zero);}
 public static uint State(){var c=Create();try{uint state;Marshal.ThrowExceptionForHR(c.GetState(IntPtr.Zero,false,out state));return state;}finally{Marshal.FinalReleaseComObject(c);}}
 public static string Title(){var c=Create();IntPtr text=IntPtr.Zero;try{Marshal.ThrowExceptionForHR(c.GetTitle(IntPtr.Zero,out text));return Marshal.PtrToStringUni(text);}finally{if(text!=IntPtr.Zero)Marshal.FreeCoTaskMem(text);Marshal.FinalReleaseComObject(c);}}
 public static string Icon(){var c=Create();IntPtr text=IntPtr.Zero;try{Marshal.ThrowExceptionForHR(c.GetIcon(IntPtr.Zero,out text));return Marshal.PtrToStringUni(text);}finally{if(text!=IntPtr.Zero)Marshal.FreeCoTaskMem(text);Marshal.FinalReleaseComObject(c);}}
 public static int Invoke(string path){var c=Create();IntPtr item=IntPtr.Zero,array=IntPtr.Zero;try{
  Guid itemId=new Guid("43826d1e-e718-42ee-bc55-a1e261c37bfe"),arrayId=new Guid("b63ea76d-1f85-456f-a19c-48159efa858b");
  Marshal.ThrowExceptionForHR(SHCreateItemFromParsingName(path,IntPtr.Zero,ref itemId,out item));
  Marshal.ThrowExceptionForHR(SHCreateShellItemArrayFromShellItem(item,ref arrayId,out array));
  return c.Invoke(array,IntPtr.Zero);
 }finally{if(array!=IntPtr.Zero)Marshal.Release(array);if(item!=IntPtr.Zero)Marshal.Release(item);Marshal.FinalReleaseComObject(c);}}
}
'@
}

function Exercise-ExplorerFolderCommand([string]$Selection,[string]$Marker,[ValidateSet('Selection','Background','Multiple','Drive')][string]$Context='Selection'){
 $result.phase='actual Explorer context '+$Context;Save $ActorStateName $result
 if(-not('OttrinContextPointer'-as[type])){Add-Type -ReferencedAssemblies System.Windows.Forms,System.Drawing -TypeDefinition 'using System;using System.Runtime.InteropServices;using System.Windows.Forms;using System.Drawing;public static class OttrinContextPointer{[DllImport("user32.dll")]static extern void mouse_event(uint flags,uint x,uint y,uint data,UIntPtr extra);public static void RightClick(int x,int y){Cursor.Position=new Point(x,y);mouse_event(8,0,0,0,UIntPtr.Zero);mouse_event(16,0,0,0,UIntPtr.Zero);}public static void Capture(string path){var r=Screen.PrimaryScreen.Bounds;using(var b=new Bitmap(r.Width,r.Height))using(var g=Graphics.FromImage(b)){g.CopyFromScreen(r.Left,r.Top,0,0,b.Size);b.Save(path,System.Drawing.Imaging.ImageFormat.Png);}}}'}
 $targets=@(@{path=$Selection;marker=$Marker});$parent=Split-Path $Selection;$second=$null
 if($Context-eq'Background'){$parent=$Selection}
 if($Context-eq'Drive'){$parent='::{20D04FE0-3AEA-1069-A2D8-08002B30309D}'}
 if($Context-eq'Multiple'){
  $second=Join-Path $parent 'second folder Ω';[IO.Directory]::CreateDirectory($second)|Out-Null
  [IO.File]::WriteAllText((Join-Path $second 'second-menu-proof.txt'),'Owned second menu fixture')
  $targets+=@{path=$second;marker='second-menu-proof.txt'}
 }
 $shell=New-Object -ComObject Shell.Application;$browser=$null;$windows=$null
 $before=@(Get-Process ottrin -ErrorAction SilentlyContinue|Where-Object {$_.Path-ieq$exe}|ForEach-Object {$_.Id})
 $daemonExe=Join-Path $package.InstallLocation 'bin\ottrin-indexd.exe'
 $daemonBefore=@(Get-Process ottrin-indexd|Where-Object {$_.Path-ieq$daemonExe}|ForEach-Object {$_.Id})
 if($daemonBefore.Count-ne1){throw 'Explorer context requires one current daemon'}
 try{
  $windows=$shell.Windows()
  foreach($existing in $windows){if($existing.Document.Folder.Self.Path-ieq$parent){throw 'Unexpected preexisting Explorer window on fixture'}}
  if($Context-eq'Drive'){$shell.Open(17)}else{$shell.Open($parent)}
  $deadline=[DateTime]::UtcNow.AddSeconds(30)
  do{
   foreach($candidate in $shell.Windows()){try{if($candidate.Document.Folder.Self.Path-ieq$parent){$browser=$candidate;break}}catch{}}
   if($browser){break};Start-Sleep -Milliseconds 200
  }while([DateTime]::UtcNow-lt$deadline)
  if(-not$browser){throw 'Owned Explorer context window did not appear'}
  $hwnd=[IntPtr][long]$browser.HWND;[PickerNative]::Place($hwnd);[PickerNative]::Focus($hwnd)
  if($Context-eq'Background'){

   $deadline=[DateTime]::UtcNow.AddSeconds(15);$view=$null
   do{
    $rootElement=[Windows.Automation.AutomationElement]::FromHandle($hwnd)
    $nodes=@($rootElement.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition))
    $anchors=@($nodes|Where-Object {$_.Current.ControlType-eq[Windows.Automation.ControlType]::ListItem-and-not$_.Current.IsOffscreen-and($_.Current.Name-ceq$Marker-or$_.Current.Name-ceq[IO.Path]::GetFileNameWithoutExtension($Marker))})
    if($anchors.Count){
     $ancestor=$anchors[0]
     for($depth=0;$depth-lt12-and$ancestor;$depth++){
      if($ancestor.Current.ControlType-eq[Windows.Automation.ControlType]::List){$view=$ancestor;break}
      $ancestor=[Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($ancestor)
     }
    }
    if($view){break};Start-Sleep -Milliseconds 200
   }while([DateTime]::UtcNow-lt$deadline)
   Save 'EXPLORER_BACKGROUND_VIEW' @{anchors=$anchors.Count;controls=@($nodes|Select-Object -First 300|ForEach-Object {$c=$_.Current;$b=$c.BoundingRectangle;@{name=$c.Name;type=$c.ControlType.ProgrammaticName;visible=(-not$c.IsOffscreen);left=$b.Left;top=$b.Top;width=$b.Width;height=$b.Height}})}
   [PickerNative]::Shot($hwnd,(Join-Path $Root 'results\explorer-background-before-menu.png'))
   if(-not$view){throw 'Owned marker has no actual Explorer items-list ancestor'}
   $bounds=$view.Current.BoundingRectangle
   if($bounds.Width-lt100-or$bounds.Height-lt100){throw 'Actual Explorer list is too small for a blank background click'}
   [OttrinContextPointer]::RightClick([int]($bounds.Right-30),[int]($bounds.Bottom-30))
  }else{
   $first=$null
   foreach($target in $targets){
    $leaf=Split-Path $target.path -Leaf
    if($Context-eq'Drive'){$drive=@($browser.Document.Folder.Items()|Where-Object {$_.Path-ieq$Selection});if($drive.Count-ne1){throw 'Expected one real drive in This PC'};$leaf=$drive[0].Name}
    $deadline=[DateTime]::UtcNow.AddSeconds(15)
    do{$items=@(UiNodes $hwnd $leaf|Where-Object {$_.Current.ControlType-eq[Windows.Automation.ControlType]::ListItem-and-not$_.Current.IsOffscreen});if($items.Count-eq1){break};Start-Sleep -Milliseconds 200}while([DateTime]::UtcNow-lt$deadline)
    if($items.Count-ne1){throw ('Explorer lacks unique selected item '+$leaf)}
    $pattern=$items[0].GetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern)
    if(-not$first){$pattern.Select();$first=$items[0]}else{$pattern.AddToSelection()}
   }
   $first.SetFocus();$selected=@($browser.Document.SelectedItems()|ForEach-Object {$_.Path})
   if($selected.Count-ne$targets.Count-or@($selected|Where-Object {$_-inotIn@($targets|ForEach-Object {$_.path})}).Count){throw 'Actual Explorer selection does not match all declared target folders'}
   [Windows.Forms.SendKeys]::SendWait('+{F10}')
  }
  $deadline=[DateTime]::UtcNow.AddSeconds(15)
  do{$commands=@([Windows.Automation.AutomationElement]::RootElement.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty,'Open in Ottrin'))|Where-Object {$_.Current.ControlType-eq[Windows.Automation.ControlType]::MenuItem-and-not$_.Current.IsOffscreen-and$_.Current.BoundingRectangle.Width-gt0-and$_.Current.BoundingRectangle.Height-gt0});if($commands.Count){break};Start-Sleep -Milliseconds 200}while([DateTime]::UtcNow-lt$deadline)
  $providers=@($commands|ForEach-Object {$c=$_.Current;$b=$c.BoundingRectangle;@{runtime_id=($_.GetRuntimeId()-join',');process=$c.ProcessId;native_window=$c.NativeWindowHandle;left=$b.Left;top=$b.Top;width=$b.Width;height=$b.Height}})
  $rows=@($commands|Group-Object {$c=$_.Current;$b=$c.BoundingRectangle;'{0}:{1}:{2}:{3}:{4}'-f$c.ProcessId,$b.Left,$b.Top,$b.Width,$b.Height})
  Save ('EXPLORER_MENU_'+$Context) @{context=$Context;targets=$targets;physical_rows=$rows.Count;providers=$providers;explorer_hwnd=$hwnd.ToInt64()}
  [OttrinContextPointer]::Capture((Join-Path $Root ('results\explorer-menu-'+$Context+'.png')))
  if($rows.Count-ne1){throw ('No unique physical Open in Ottrin command for '+$Context)}
  $commands=@($rows[0].Group|Where-Object {$_.Current.IsEnabled});if(-not$commands.Count){throw 'Explorer command is disabled'}
  $commands[0].GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke()
  $deadline=[DateTime]::UtcNow.AddSeconds(60);$mapped=@()
  do{
   $mains=@()
   foreach($candidate in @(Get-Process ottrin -ErrorAction SilentlyContinue|Where-Object {$_.Path-ieq$exe-and$_.Id-notin$before})){
    $native=Get-CimInstance Win32_Process -Filter ('ProcessId='+$candidate.Id);if(-not$native){continue}
    $argv=[PackagedShellQA]::Arguments($native.CommandLine)
    if($argv.Count-eq3-and[IO.Path]::GetFullPath($argv[0])-ieq[IO.Path]::GetFullPath($exe)-and$argv[1]-ceq'--'-and$argv[2]-cin@($targets|ForEach-Object {$_.path})){
     $null=$candidate.Handle;$mains+=@{process=$candidate;path=$argv[2];window=[PickerNative]::Window($candidate.Id,'Ottrin')}
    }elseif(-not($argv.Count-eq3-and$argv[1]-ceq'index'-and$argv[2]-ceq'ensure')){throw 'Unexpected process from Explorer context invocation'}
   }
   if($mains.Count-gt$targets.Count){throw 'Explorer command duplicated its main processes'}
   foreach($target in $targets){if(@($mains|Where-Object {$_.path-ceq$target.path}).Count-gt1){throw 'Duplicate main for one selected folder'}}
   if($mains.Count-eq$targets.Count-and@($mains|Where-Object {$_.window-eq[IntPtr]::Zero}).Count-eq0){$mapped=$mains;break};Start-Sleep -Milliseconds 200
  }while([DateTime]::UtcNow-lt$deadline)
  if($mapped.Count-ne$targets.Count){throw 'Explorer context did not map exactly one main per target'}
  $foreground=[PickerNative]::GetForegroundWindow();$automaticForeground=@($mapped|Where-Object {$_.window-eq$foreground}).Count-eq1
  if(-not$automaticForeground){throw 'No context-launched main acquired foreground before QA input'}
  $proofs=@()
  foreach($entry in $mapped){
   $child=$entry.process;$window=$entry.window;[uint32]$length=0
   if([PackageIdentityQA]::GetPackageFullName($child.Handle,[ref]$length,$null)-ne122){throw 'Context main package identity unavailable'}
   $name=[Text.StringBuilder]::new([int]$length)
   if([PackageIdentityQA]::GetPackageFullName($child.Handle,[ref]$length,$name)-ne0-or$name.ToString()-ne$package.PackageFullName-or[InputDesktopQA]::Elevated($child.Handle)){throw 'Context main must have ordinary current package identity'}
   $identity=Invoke-CimMethod -InputObject (Get-CimInstance Win32_Process -Filter ('ProcessId='+$child.Id)) -MethodName GetOwnerSid
   if($identity.Sid-ne$owner.sid){throw 'Context main user mismatch'}
   $target=@($targets|Where-Object {$_.path-ceq$entry.path})[0];$deadline=[DateTime]::UtcNow.AddSeconds(20)
   do{$markers=@(UiNodes $window $target.marker|Where-Object {-not$_.Current.IsOffscreen});if($markers.Count){break};Start-Sleep -Milliseconds 100}while([DateTime]::UtcNow-lt$deadline)
   if(-not$markers.Count){throw ('Context main lacks expected folder content '+$target.marker)}
   [PickerNative]::Place($window);[PickerNative]::Focus($window);[PickerNative]::Shot($window,(Join-Path $Root ('results\explorer-invoked-'+$Context+'-'+$proofs.Count+'.png')))
   [PackageIdentityQA]::PostMessageW($window,0x10,[IntPtr]::Zero,[IntPtr]::Zero)|Out-Null
   $closed=$child.WaitForExit(15000);if($closed){$child.WaitForExit()};$exitCode=if($closed){$child.ExitCode}else{$null}
   $proofs+=@{pid=$child.Id;path=$entry.path;marker=$target.marker;package=$name.ToString();sid=$identity.Sid;closed=$closed;exit_code=$exitCode}
   Save ('EXPLORER_PROCESSES_'+$Context) @{context=$Context;automatic_foreground=$automaticForeground;processes=$proofs}
   if(-not$closed-or$exitCode-ne0){throw 'Context main did not close normally'}
  }
  $after=@(Get-Process ottrin-indexd|Where-Object {$_.Path-ieq$daemonExe}|ForEach-Object {$_.Id})
  if($after.Count-ne1-or$after[0]-ne$daemonBefore[0]){throw 'Explorer context changed the running daemon'}
  return @{status='pass actual Explorer context';context=$Context;targets=$targets;processes=$proofs;automatic_foreground=$automaticForeground;daemon_before=$daemonBefore;daemon_after=$after;providers=$providers}
 }finally{
  foreach($process in @(Get-Process ottrin -ErrorAction SilentlyContinue|Where-Object {$_.Path-ieq$exe-and$_.Id-notin$before})){if(-not$process.HasExited){$process.Kill();$null=$process.WaitForExit(5000)}}
  if($browser){if($browser.Document.Folder.Self.Path-ine$parent){throw 'Owned Explorer window navigated elsewhere'};$browser.Quit();[Runtime.InteropServices.Marshal]::FinalReleaseComObject($browser)|Out-Null}
  if($windows){[Runtime.InteropServices.Marshal]::FinalReleaseComObject($windows)|Out-Null};[Runtime.InteropServices.Marshal]::FinalReleaseComObject($shell)|Out-Null
  if($second){$markerPath=Join-Path $second 'second-menu-proof.txt';if([IO.File]::ReadAllText($markerPath)-cne'Owned second menu fixture'){throw 'Owned second fixture changed; preserved'};[IO.File]::Delete($markerPath);[IO.Directory]::Delete($second)}
 }
}

function Exercise-PackagedFolderCommand($App,$Settings){
 $result.phase='external ordinary-user packaged COM activation';Save $ActorStateName $result
 Initialize-ShellProbe
 if([PackagedShellQA]::HostIdentity()-ne15700){throw 'External shell probe must have no package identity'}
 if([PackagedShellQA]::Title()-cne'Open in Ottrin'){throw 'Actual registered COM command title mismatch'}
 $expectedIcon=(Join-Path $package.InstallLocation 'bin\ottrin.exe')+',0'
 $icon=[PackagedShellQA]::Icon()
 if(-not$icon.EndsWith(',0')-or[IO.Path]::GetFullPath($icon.Substring(0,$icon.Length-2))-ine[IO.Path]::GetFullPath($expectedIcon.Substring(0,$expectedIcon.Length-2))){throw 'COM command did not resolve its actual package icon'}
 $initialEnabled=$Mode-eq'Updated'-and$owner.package.previous.version-eq'0.1.0.24'
 if([PackagedShellQA]::State()-ne$(if($initialEnabled){0}else{2})){throw 'Folder command did not retain its declared initial/update preference'}
 $proof=@{status='running';initial_enabled=$initialEnabled;host_package_identity_code=15700;title='Open in Ottrin';icon=$expectedIcon;actions=@();scope='Actual ordinary-user package COM activation/visibility/invocation; real Explorer menu surface and automatic foreground behavior remain separate'}
 $selection=Join-Path $Root ('shell-selection\-- folder with spaces '+[char]0x03a9+[char]::ConvertFromUtf32(0x10437))
 [IO.Directory]::CreateDirectory($selection)|Out-Null
 $marker=Join-Path $selection 'menu-invocation-proof.txt';[IO.File]::WriteAllText($marker,'Owned shell invocation fixture')
 $before=@(Get-Process ottrin -ErrorAction SilentlyContinue|Where-Object {$_.Path-ieq$exe}|ForEach-Object {$_.Id})
 $daemonPath=Join-Path $package.InstallLocation 'bin\ottrin-indexd.exe'
 $daemonBefore=@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue|Where-Object {$_.Path-ieq$daemonPath}|ForEach-Object {$_.Id})
 if($daemonBefore.Count-ne1){throw 'Folder command check requires the existing ready package daemon'}
 $proof.daemon_pids_before=$daemonBefore
 $child=$null;$childWindow=[IntPtr]::Zero
 try{
  for($scroll=0;$scroll-lt8;$scroll++){
   $buttons=@(UiNodes $Settings 'Enable folder menu actions'|Where-Object {-not$_.Current.IsOffscreen})
   if($buttons.Count-eq1){break};[PickerNative]::Wheel($Settings,-240);Start-Sleep -Milliseconds 300
  }
  Save 'FOLDER_COMMAND' $proof
  $requested=if($initialEnabled){@($false,$true)}else{@($true,$false,$true)}
  foreach($enabled in $requested){
   $result.phase='packaged folder command visibility '+$enabled;Save $ActorStateName $result
   UiInvoke $Settings $(if($enabled){'Enable folder menu actions'}else{'Remove folder menu actions'})
   $expected=if($enabled){0}else{2};$deadline=[DateTime]::UtcNow.AddSeconds(15)
   do{$state=[PackagedShellQA]::State();if($state-eq$expected){break};Start-Sleep -Milliseconds 100}while([DateTime]::UtcNow-lt$deadline)
   if($state-ne$expected){throw 'Actual COM command did not follow the UI visibility preference'}
   $label=if($enabled){'Enabled — Open in Ottrin is available in folder menus'}else{'Off — Open in Ottrin is hidden from folder menus'}
   $node=UiNode $Settings $label ([Windows.Automation.ControlType]::Text)
   $frame=[Windows.Automation.AutomationElement]::FromHandle($Settings).Current.BoundingRectangle;$rect=$node.Current.BoundingRectangle
   if($rect.Left-lt$frame.Left-or$rect.Right-gt$frame.Right-or$rect.Top-lt$frame.Top-or$rect.Bottom-gt$frame.Bottom){throw 'Complete folder status must fit inside Settings'}
   if(-not$enabled){
    if([PackagedShellQA]::Invoke($selection)-ne-2147024891){throw 'Disabled command must reject invocation with E_ACCESSDENIED'}
    if(@(Get-Process ottrin -ErrorAction SilentlyContinue|Where-Object {$_.Path-ieq$exe-and$_.Id-notin$before}).Count){throw 'Disabled invocation launched a main window'}
   }
   [PickerNative]::Focus($Settings)
   [PickerNative]::Shot($Settings,(Join-Path $Root ('results\msix-folder-'+$(if($enabled){'enabled'}else{'disabled'})+'.png')))
   $proof.actions+=@{enabled=$enabled;com_state=$state;complete_status_bounds=@{left=$rect.Left;top=$rect.Top;width=$rect.Width;height=$rect.Height}}
   Save 'FOLDER_COMMAND' $proof
  }
  $result.phase='packaged COM selected-folder invocation';Save $ActorStateName $result
  $code=[PackagedShellQA]::Invoke($selection);if($code-ne0){throw ('Enabled COM invocation failed: '+$code)}
  $deadline=[DateTime]::UtcNow.AddSeconds(60)
  do{
   $new=@(Get-Process ottrin -ErrorAction SilentlyContinue|Where-Object {$_.Path-ieq$exe-and$_.Id-notin$before})
   $main=@();$bootstrap=@();$observed=@()
   foreach($candidate in $new){
    $native=Get-CimInstance Win32_Process -Filter ('ProcessId='+$candidate.Id)
    if(-not$native){continue}
    $arguments=[PackagedShellQA]::Arguments($native.CommandLine)
    $observation=@{pid=$candidate.Id;parent=[uint32]$native.ParentProcessId;arguments=$arguments}
    $observed+=,$observation
    if($arguments.Count-eq3-and[IO.Path]::GetFullPath($arguments[0])-ieq[IO.Path]::GetFullPath($exe)-and$arguments[1]-ceq'--'-and$arguments[2]-ceq$selection){$main+=,$candidate}
    elseif($arguments.Count-eq3-and[IO.Path]::GetFullPath($arguments[0])-ieq[IO.Path]::GetFullPath($exe)-and$arguments[1]-ceq'index'-and$arguments[2]-ceq'ensure'){$bootstrap+=,$observation}
    else{Save 'FOLDER_COMMAND_PROCESSES' $observed;throw 'Unexpected process arguments after selected-folder invocation'}
   }
   Save 'FOLDER_COMMAND_PROCESSES' $observed
   if($main.Count-gt1){throw 'One selected folder must launch exactly one main process'}
   if($bootstrap.Count-gt1){throw 'One main window must not duplicate its index bootstrap'}
   foreach($startup in $bootstrap){if($startup.parent-notin@($main|ForEach-Object {$_.Id})){throw 'Index bootstrap must be owned by the selected-folder main process'}}
   if($main.Count-eq1){$child=$main[0];$childWindow=[PickerNative]::Window($child.Id,'Ottrin');if($childWindow-ne[IntPtr]::Zero){$proof.invocation_processes=$observed;break}}
   Start-Sleep -Milliseconds 200
  }while([DateTime]::UtcNow-lt$deadline)
  if(-not$child-or$childWindow-eq[IntPtr]::Zero){throw 'COM invocation did not map a main window'}
  [uint32]$length=0
  if([PackageIdentityQA]::GetPackageFullName($child.Handle,[ref]$length,$null)-ne122){throw 'Invoked main process package identity unavailable'}
  $name=[Text.StringBuilder]::new([int]$length)
  if([PackageIdentityQA]::GetPackageFullName($child.Handle,[ref]$length,$name)-ne0-or$name.ToString()-ne$package.PackageFullName-or[InputDesktopQA]::Elevated($child.Handle)){throw 'Invoked main process must belong to the ordinary installed package'}
  $menu=UiNode $childWindow 'Menu' ([Windows.Automation.ControlType]::Button)
  $deadline=[DateTime]::UtcNow.AddSeconds(20)
  do{$markers=@(UiNodes $childWindow 'menu-invocation-proof.txt'|Where-Object {-not$_.Current.IsOffscreen-and$_.Current.BoundingRectangle.Width-gt0-and$_.Current.BoundingRectangle.Height-gt0});if($markers.Count){break};Start-Sleep -Milliseconds 100}while([DateTime]::UtcNow-lt$deadline)
  if(-not$markers.Count){throw 'Invoked folder does not show its owned marker file'}
  [PickerNative]::Place($childWindow);$menu.SetFocus();[PickerNative]::Focus($childWindow)
  [PickerNative]::Shot($childWindow,(Join-Path $Root 'results\msix-folder-invoked.png'))
  $proof.invocation=@{exit=$code;pid=$child.Id;package=$name.ToString();selected_folder=$selection;visible_marker='menu-invocation-proof.txt';ordinary=$true;capture_focus='Explicit private QA focus; no automatic foreground acceptance'}
  [PackageIdentityQA]::PostMessageW($childWindow,0x10,[IntPtr]::Zero,[IntPtr]::Zero)|Out-Null
  if(-not$child.WaitForExit(15000)-or$child.ExitCode-ne0){throw 'Invoked main window did not close normally'}
  $daemonAfter=@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue|Where-Object {$_.Path-ieq$daemonPath}|ForEach-Object {$_.Id})
  if($daemonAfter.Count-ne1-or$daemonAfter[0]-ne$daemonBefore[0]){throw 'Folder commands or closing their main window restarted the index daemon'}
  $proof.daemon_pids_after=$daemonAfter
  $proof.contexts=@()
  foreach($context in @('Background','Multiple','Drive')){
   $target=if($context-eq'Drive'){[IO.Path]::GetPathRoot($env:SystemRoot)}else{$selection}
   $expected=if($context-eq'Drive'){'Windows'}else{'menu-invocation-proof.txt'}
   $proof.contexts+=Exercise-ExplorerFolderCommand $target $expected $context
   Save 'FOLDER_COMMAND' $proof
  }
  $proof.status='pass native packaged COM activation, UI preference and selected-folder invocation';$result.folder_command=$proof;Save 'FOLDER_COMMAND' $proof;Save $ActorStateName $result
 }finally{
  foreach($process in @(Get-Process ottrin -ErrorAction SilentlyContinue|Where-Object {$_.Path-ieq$exe-and$_.Id-notin$before})){
   if(-not$process.HasExited){$process.Kill();$null=$process.WaitForExit(5000)}
  }
  if(Test-Path $marker){if([IO.File]::ReadAllText($marker)-cne'Owned shell invocation fixture'){throw 'Owned shell fixture changed; retained'};Remove-Item -LiteralPath $marker}
  if(Test-Path $selection){[IO.Directory]::Delete($selection)}
 }
}

function Exercise-StartupCheckbox($App,$Window){
 $daemonPath=Join-Path $package.InstallLocation 'bin\ottrin-indexd.exe'
 $before=@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue|Where-Object {$_.Path-ieq$daemonPath}|ForEach-Object {$_.Id}|Sort-Object)
 if($before.Count-ne1){throw 'Packaged startup checkbox requires one existing daemon'}
 UiInvoke $Window 'Menu';UiInvoke $Window ('Settings'+[char]0x2026)
 $deadline=[DateTime]::UtcNow.AddSeconds(20);$settings=[IntPtr]::Zero
 do{$settings=[PickerNative]::Window($App.Id,'Ottrin Settings');if($settings-ne[IntPtr]::Zero){break};Start-Sleep -Milliseconds 100}while([DateTime]::UtcNow-lt$deadline)
 if($settings-eq[IntPtr]::Zero){throw 'Packaged Settings did not map'}
 try{
  if(-not[PackageIdentityQA]::SetWindowPos($settings,[IntPtr]::Zero,0,0,0,0,0x13)){throw 'Cannot bring owned Settings above overlapping viewport'}
  [PickerNative]::Focus($settings)
  (UiNode $settings 'Services' ([Windows.Automation.ControlType]::RadioButton)).GetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern).Select()
  $checkbox=$null
  for($scroll=0;$scroll-lt18;$scroll++){
   $matches=@(UiNodes $settings 'Start index automatically at sign-in'|Where-Object {$_.Current.ControlType-eq[Windows.Automation.ControlType]::CheckBox -and -not$_.Current.IsOffscreen})
   if($matches.Count-eq1){$checkbox=$matches[0];break};[PickerNative]::Wheel($settings,-480);Start-Sleep -Milliseconds 200
  }
  if(-not$checkbox){throw 'Actual MSIX startup checkbox is not visible'}
  if($checkbox.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern).Current.ToggleState-ne[Windows.Automation.ToggleState]::On){throw 'Initial MSIX startup choice must be enabled'}
  $proof=@()
  function CheckboxTrace($Stage){
   $fields=@();foreach($key in $result.Keys){$value=$result[$key];$fields+=@{key=[string]$key;type=$(if($null-eq$value){'null'}else{$value.GetType().FullName})}}
   Save 'MSIX_CHECKBOX_TRACE' @{stage=[string]$Stage;fields=$fields;proof_count=[int]$proof.Count;proof_types=@($proof|ForEach-Object {$_.GetType().FullName});before_types=@($before|ForEach-Object {$_.GetType().FullName});after_types=@($after|ForEach-Object {$_.GetType().FullName})}
  }
  foreach($enabled in @($false,$true)){
   $checkbox=UiNode $settings 'Start index automatically at sign-in' ([Windows.Automation.ControlType]::CheckBox)
   if(-not$checkbox.Current.IsEnabled){throw 'Actual package startup checkbox is unexpectedly disabled'}
   $checkbox.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern).Toggle()
   $deadline=[DateTime]::UtcNow.AddSeconds(45);$expected=if($enabled){[Windows.Automation.ToggleState]::On}else{[Windows.Automation.ToggleState]::Off}
   $completion='Last completed action: Automatic startup at sign-in '+$(if($enabled){'enabled'}else{'disabled'})+'. Current indexing is unchanged.'
   do{
    $checkbox=UiNode $settings 'Start index automatically at sign-in' ([Windows.Automation.ControlType]::CheckBox)
    $completed=@(UiNodes $settings $completion|Where-Object {-not$_.Current.IsOffscreen}).Count-eq1
    if($completed-and$checkbox.Current.IsEnabled-and$checkbox.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern).Current.ToggleState-eq$expected){break}
    Start-Sleep -Milliseconds 200
   }while([DateTime]::UtcNow-lt$deadline)
   if(-not$completed-or-not$checkbox.Current.IsEnabled-or$checkbox.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern).Current.ToggleState-ne$expected){throw 'Packaged startup action did not complete and settle at requested state'}
   $label=if($enabled){'enabled'}else{'disabled'}
   $process=Invoke-OwnCLI @('index','service-status') ('checkbox-'+$label+'.stdout') ('checkbox-'+$label+'.stderr')
   $text=Get-Content (Join-Path $Root ('results\checkbox-'+$label+'.stdout')) -Raw
   if($process.ExitCode-ne0-or$text-notmatch'Windows Startup apps'-or$text-notmatch$(if($enabled){'Enabled at login'}else{'enabled=false'})){throw 'Native checkbox and package startup registration disagree'}
   $after=@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue|Where-Object {$_.Path-ieq$daemonPath}|ForEach-Object {$_.Id}|Sort-Object)
   if(($before-join',')-ne($after-join',')){throw 'Startup checkbox changed the running daemon'}
   # CLI inspection can move focus back to the main viewport. Capture the
   # actual Settings surface after restoring foreground and allowing repaint.
   if(-not[PackageIdentityQA]::SetWindowPos($settings,[IntPtr]::Zero,0,0,0,0,0x13)){throw 'Cannot bring owned Settings above overlapping viewport'}
   [PickerNative]::Focus($settings);Start-Sleep -Milliseconds 300
   if([PickerNative]::GetForegroundWindow()-ne$settings){throw 'Startup screenshot requires Settings foreground'}
   [PickerNative]::Shot($settings,(Join-Path $Root ('results\msix-startup-'+$label+'.png')))
   CheckboxTrace ('before proof '+$label)
   $proof+=@{enabled=[bool]$enabled;daemon_pids_before=[int[]]$before;daemon_pids_after=[int[]]$after;backend=[string]$text}
   CheckboxTrace ('after proof '+$label)
  }
  CheckboxTrace 'before result assignment'
  $result.msix_startup_checkbox=@{status='pass actual installed-package checkbox';actions=$proof;package_identity=[string]$result.actual_process_package}
  CheckboxTrace 'before result serialization'
  Save $ActorStateName $result
  CheckboxTrace 'after result serialization'

  if([string]$package.Version-eq$owner.package.version){Exercise-PackagedFolderCommand $App $settings}else{
   if(-not$owner.package.previous-or[string]$package.Version-ne$owner.package.previous.version-or$owner.package.previous.version-ne'0.1.0.24'){throw 'Undeclared baseline package'}
   Initialize-ShellProbe
   if([PackagedShellQA]::HostIdentity()-ne15700-or[PackagedShellQA]::State()-ne2){throw 'Previous package must start with its command hidden'}
   UiInvoke $settings 'Enable folder menu actions';$deadline=[DateTime]::UtcNow.AddSeconds(15)
   do{$state=[PackagedShellQA]::State();if($state-eq0){break};Start-Sleep -Milliseconds 100}while([DateTime]::UtcNow-lt$deadline)
   if($state-ne0){throw 'Previous package folder command did not enable'}
   $result.previous_folder_command=@{enabled=$true;com_state=$state;version=[string]$package.Version;package=$package.PackageFullName}
   Save $ActorStateName $result
  }

 }catch{
  [PickerNative]::Shot($settings,(Join-Path $Root 'results\msix-startup-failure.png'));throw
 }finally{
  if($settings-ne[IntPtr]::Zero){[PackageIdentityQA]::PostMessageW($settings,0x10,[IntPtr]::Zero,[IntPtr]::Zero)|Out-Null;Start-Sleep -Milliseconds 300}
 }
}
function Exercise-MainWindow([switch]$StartupCheckbox){
 $readyDeadline=[DateTime]::UtcNow.AddMinutes(2)
 while([InputDesktopQA]::Name()-ne'Default'-and[DateTime]::UtcNow-lt$readyDeadline){Start-Sleep -Seconds 1}
 $result.input_desktop=[InputDesktopQA]::Name();Save $ActorStateName $result
 & ([ScriptBlock]::Create([IO.File]::ReadAllText((Join-Path $Root 'native-controls.ps1'))));[PickerNative]::Init()
 Start-Process $exe -ArgumentList @('--renderer','opengl') -WindowStyle Normal|Out-Null
 $deadline=[DateTime]::UtcNow.AddSeconds(60);$app=$null;$window=[IntPtr]::Zero
 while([DateTime]::UtcNow-lt$deadline){
  foreach($candidate in @(Get-Process ottrin -ErrorAction SilentlyContinue|Where-Object {$_.Path-ieq$exe})){
   $found=[PickerNative]::Window($candidate.Id,'Ottrin')
   if($found-ne[IntPtr]::Zero){$app=$candidate;$window=$found;break}
  }
  if($app){break};Start-Sleep -Milliseconds 200
 }
 if(-not$app){throw 'Actual packaged main window did not map'}
 $processOwner=Invoke-CimMethod -InputObject (Get-CimInstance Win32_Process -Filter ("ProcessId="+$app.Id)) -MethodName GetOwnerSid
 if($processOwner.Sid-ne$owner.sid){throw 'Main window belongs to another identity'}

 [uint32]$length=0;$errorCode=[PackageIdentityQA]::GetPackageFullName($app.Handle,[ref]$length,$null)
 if($errorCode-ne122){throw 'Main process has no queryable package identity'}
 $buffer=[Text.StringBuilder]::new([int]$length)
 if([PackageIdentityQA]::GetPackageFullName($app.Handle,[ref]$length,$buffer)-ne0-or$buffer.ToString()-ne$package.PackageFullName){throw 'Main process package identity mismatch'}
 $result.actual_process_package=$buffer.ToString()
 $result.actual_process_elevated=[InputDesktopQA]::Elevated($app.Handle)
 if($result.actual_process_elevated){throw 'Actual main process must have an ordinary non-elevated token'}
 $result.window_diagnostics=@{console_session=[InputDesktopQA]::WTSGetActiveConsoleSessionId();process_session=$app.SessionId;minimized=[InputDesktopQA]::IsIconic($window);cloaked=[InputDesktopQA]::Cloaked($window);foreground=[InputDesktopQA]::GetForegroundWindow().ToInt64();window=$window.ToInt64();enable_lua=(Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System').EnableLUA}
 [InputDesktopQA]::ShowWindow($window,9)|Out-Null
 [PickerNative]::Place($window)
 $guiFailure=$null
 try {
  if($result.input_desktop-ne'Default'){throw 'Fresh-login input desktop not ready'}
  # A visible HWND can precede the first accessible/painted application frame.
  # Require the real Menu control before native activation; do not infer GUI
  # readiness from window creation alone.
  $menu=UiNode $window 'Menu' ([Windows.Automation.ControlType]::Button)
  [PickerNative]::Shot($window,(Join-Path $Root 'results\mapped-main-before-focus.png'))
  $focusDeadline=[DateTime]::UtcNow.AddSeconds(15);$focusAttempts=@()
  while($true){
   try{
    if([PickerNative]::Window($app.Id,'Ottrin')-ne$window){throw 'Owned window identity changed before activation'}
    if(-not[PackageIdentityQA]::SetWindowPos($window,[IntPtr]::Zero,0,0,0,0,0x13)){throw 'Cannot raise owned main window'}
    # Request focus through the actual accessible application control before
    # native activation; preserve the foreground-window assertion below.
    $menu.SetFocus()
    [PickerNative]::Focus($window);break
   }catch{
    $foreground=[PickerNative]::GetForegroundWindow();$focusAttempts+=@{error=$_.Exception.Message;geometry=[PickerNative]::Geometry($window);foreground=$foreground.ToInt64();foreground_title=[PickerNative]::Title($foreground);cursor_x=[Windows.Forms.Cursor]::Position.X;cursor_y=[Windows.Forms.Cursor]::Position.Y}
    $result.focus_attempts=$focusAttempts;Save $ActorStateName $result
    if([DateTime]::UtcNow-ge$focusDeadline){throw}
    Start-Sleep -Milliseconds 250
   }
  }
  Start-Sleep -Milliseconds 500
  if($StartupCheckbox){Exercise-StartupCheckbox $app $window}
  [PickerNative]::Shot($window,(Join-Path $Root ('results\'+$(if($Mode-eq'Updated'){'updated-main.png'}else{'main.png'}))));$result.gui_focus_and_capture='pass'
 }catch{$result.gui_focus_and_capture='failed';$result.gui_error=$_.Exception.Message;$guiFailure=$_;try{[PickerNative]::Shot($window,(Join-Path $Root 'results\focus-failure.png'));$result.focus_failure_capture='focus-failure.png'}catch{$result.focus_failure_capture_error=$_.Exception.Message}}
 $result.main=@{pid=$app.Id;path=$app.Path;title=[PickerNative]::Title($window);binary_sha256=(Get-FileHash $app.Path).Hash};Save $ActorStateName $result
 try {
  if([PickerNative]::Window($app.Id,'Ottrin')-ne$window){throw 'Owned main window identity changed'}
  if(-not[PackageIdentityQA]::PostMessageW($window,0x10,[IntPtr]::Zero,[IntPtr]::Zero)){throw 'Normal window close was refused'}
  if(-not$app.WaitForExit(15000)){throw 'Main window did not close'};$app.WaitForExit();if($app.ExitCode-ne0){throw 'Main window exited abnormally'};$result.normal_close_exit=0
 }catch{$result.normal_close_error=$_.Exception.Message;throw}
 if($guiFailure){throw $guiFailure}
}
if($Mode-in@('Install','Finalize')){
 $result=@{status='running';scope=$owner.scope;source_revision=$owner.package.source_revision;actor_sid=$owner.sid}
 try {
  if(-not$admin){throw 'Unsigned development deployment requires the explicit administrative QA token'}
  if($Mode-eq'Finalize'){
   $saved=Read-State (Join-Path $Root 'results\RESULT.json')
   $result=@{};foreach($property in $saved.PSObject.Properties){$result[$property.Name]=$property.Value}
   $actor=$result.actor;$package=Get-AppxPackage -Name 'Ottrin.Development'
   $deadline=[DateTime]::UtcNow.AddMinutes(5);$loginPath=Join-Path $Root 'results\LOGIN.json'
   do{
    if(Test-Path $loginPath){$login=Read-State $loginPath;if($login.status-ne'running'){break}}
    if([DateTime]::UtcNow-ge$deadline){throw 'Ordinary actual-login observer did not finish'};Start-Sleep -Milliseconds 250
   }while($true)
   if($result.status-ne'awaiting actual login'-or$login.status-ne'pass'-or$login.boot_ticks-eq$result.before_login_boot_ticks){throw 'Actual new-login startup-choice proof required before finalization'}
   $result.login=$login
  }else{
  if(@(Get-AppxPackage -Name 'Ottrin.Development').Count){throw 'Unexpected existing development package'}
  $initial=if($owner.package.previous){'Previous.msix'}else{'Ottrin.msix'}
  Add-AppxPackage -Path (Join-Path $Root $initial) -AllowUnsigned
  $package=Get-AppxPackage -Name 'Ottrin.Development';if(-not$package){throw 'Package registration missing'}
  $result.deployment=@{full_name=$package.PackageFullName;family=$package.PackageFamilyName;location=$package.InstallLocation;version=[string]$package.Version};Save 'DEPLOYMENT' $result
  Save 'DESKTOP_WAITING' @{status='waiting for owned ordinary desktop';sid=$owner.sid;nonce=$owner.nonce;scope='Complete the disposable account privacy setup through the private console; do not alter security or OOBE policies'}
  $deadline=[DateTime]::UtcNow.AddMinutes(8);$readyPath=Join-Path $Root 'results\DESKTOP_READY.json'
  while(-not(Test-Path $readyPath)){if([DateTime]::UtcNow-ge$deadline){throw 'Owned first-login desktop was not made ready'};Start-Sleep -Milliseconds 250}
  $ready=Read-State $readyPath
  if($ready.sid-ne$owner.sid-or$ready.nonce-ne$owner.nonce){throw 'Desktop readiness belongs to another fixture'}
  Start-ScheduledTask -TaskName $owner.actor_task;$deadline=[DateTime]::UtcNow.AddMinutes(8)
  while([DateTime]::UtcNow-lt$deadline){$path=Join-Path $Root 'results\ACTOR.json';if(Test-Path $path){$actor=Read-State $path;if($actor.status-ne'running'){break}};Start-Sleep -Seconds 1}
  if(-not$actor-or$actor.status-eq'running'){throw 'Ordinary runtime actor timed out'}
  $result.actor=$actor
  $events=@(Get-WinEvent -FilterHashtable @{LogName='Application';Id=1000;StartTime=(Get-Date).AddMinutes(-10)} -ErrorAction SilentlyContinue|Where-Object {$_.Message.IndexOf($package.InstallLocation,[StringComparison]::OrdinalIgnoreCase)-ge0}|Select-Object TimeCreated,Id,Message)
  Save 'CRASH_EVENTS' @{events=$events;scope='Only events naming this owned package installation'}
  if($actor.status-eq'pass'-and$owner.package.previous){
   $result.pre_update_actor=$actor;$before=$package
   if([string]$before.Version-ne$owner.package.previous.version){throw 'Unexpected installed previous package version'}
   $oldProcesses=@(Get-Process ottrin,ottrin-indexd -ErrorAction SilentlyContinue|Where-Object {$_.Path-and$_.Path.StartsWith($before.InstallLocation,[StringComparison]::OrdinalIgnoreCase)})
   $expectedDaemonCount=if($owner.package.startup_enabled){1}else{0}
   if(@($oldProcesses|Where-Object {$_.ProcessName-eq'ottrin-indexd'}).Count-ne$expectedDaemonCount){throw 'Pre-update daemon state does not match the declared startup choice'}
   Save 'BEFORE_UPDATE' $result
   Add-AppxPackage -Path (Join-Path $Root 'Ottrin.msix') -AllowUnsigned -ForceApplicationShutdown
   $package=Get-AppxPackage -Name 'Ottrin.Development'
   if([string]$package.Version-ne$owner.package.version-or$package.PackageFamilyName-ne$before.PackageFamilyName-or$package.PackageFullName-eq$before.PackageFullName){throw 'Actual same-identity version update failed'}
   $remaining=@(Get-Process ottrin,ottrin-indexd -ErrorAction SilentlyContinue|Where-Object {$_.Path-and$_.Path.StartsWith($before.InstallLocation,[StringComparison]::OrdinalIgnoreCase)})
   if($remaining.Count){throw 'Old package processes survived Windows update shutdown'}
   $result.update=@{before=$before.PackageFullName;after=$package.PackageFullName;family=$package.PackageFamilyName;old_processes_removed=$true;startup_enabled=$owner.package.startup_enabled;previous_daemon_count=$expectedDaemonCount;old_pids=@($oldProcesses|ForEach-Object {$_.Id})}
   Save 'UPDATE' $result
   Start-ScheduledTask -TaskName $owner.updated_task;$deadline=[DateTime]::UtcNow.AddMinutes(5)
   do{
    $path=Join-Path $Root 'results\UPDATED.json'
    if(Test-Path $path){$updated=Read-State $path;if($updated.status-ne'running'){break}}
    if([DateTime]::UtcNow-ge$deadline){throw 'Ordinary post-update actor timed out'};Start-Sleep -Milliseconds 250
   }while($true)
   $result.updated_actor=$updated;$actor=$updated;$result.actor=$actor
   if($actor.status-ne'pass'){throw 'Actual post-update ordinary runtime failed'}
  }
  if($actor.status-eq'pass'){
   # Only Windows may start the daemon on the next login. No Ottrin CLI is scheduled.
   $trigger=New-ScheduledTaskTrigger -AtLogOn -User ($env:COMPUTERNAME+'\'+$owner.account)
   Set-ScheduledTask -TaskName $owner.login_task -Trigger $trigger|Out-Null
   Set-ScheduledTask -TaskName $owner.finalizer_task -Trigger $trigger|Out-Null
   Unregister-ScheduledTask -TaskName $owner.installer_task -Confirm:$false
   $result.before_login_boot_ticks=(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().Ticks
   $result.status='awaiting actual login';Save 'RESULT' $result
   exit 0
  }
  }
  Remove-AppxPackage -Package $package.PackageFullName
  $result.package_removed=(@(Get-AppxPackage -Name 'Ottrin.Development').Count-eq0)
  if($actor.status-eq'pass'){
   Start-ScheduledTask -TaskName $owner.removed_task;$deadline=[DateTime]::UtcNow.AddSeconds(45)
   do{$path=Join-Path $Root 'results\AFTER_REMOVAL.json';if(Test-Path $path){$removed=Read-State $path;if($removed.status-ne'running'){break}};Start-Sleep -Milliseconds 200}while([DateTime]::UtcNow-lt$deadline)
   if(-not$removed-or$removed.status-ne'pass'){throw 'Ordinary-user COM removal verification failed'};$result.com_unregistration=$removed
  }
  $deadline=[DateTime]::UtcNow.AddSeconds(15)
  do{$live=@(Get-Process ottrin,ottrin-indexd -ErrorAction SilentlyContinue|Where-Object {$_.Path-and$_.Path.StartsWith($package.InstallLocation,[StringComparison]::OrdinalIgnoreCase)});if($live.Count-eq0){break};Start-Sleep -Milliseconds 200}while([DateTime]::UtcNow-lt$deadline)
  $result.package_processes_removed=($live.Count-eq0)
  $config=Join-Path $env:APPDATA 'ottrin\config.json'
  $result.preferences_preserved=($actor.preferences_before_removal-and(Test-Path $config)-and(Get-FileHash $config).Hash-eq$actor.preferences_before_removal)
  $tasks=@(Get-ScheduledTask|Where-Object {
   if($_.TaskName-notlike'Ottrin Index*'){return $false}
   $principal=[string]$_.Principal.UserId
   $sid=if($principal-like'S-1-*'){$principal}else{([Security.Principal.NTAccount]::new($principal)).Translate([Security.Principal.SecurityIdentifier]).Value}
   $sid-eq$owner.sid
  })
  $result.remaining_index_tasks=@($tasks|ForEach-Object {@{name=$_.TaskName;enabled=$_.Settings.Enabled;actions=@($_.Actions|ForEach-Object {$_.Execute})}})
  $result.fixture_preserved=(Get-Content (Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'OttrinMSIXFixture\preserve.txt') -Raw)-eq"Preserve owned MSIX fixture`n"
  $result.status=if($actor.status-eq'pass'-and$result.package_removed-and$result.package_processes_removed-and$result.preferences_preserved-and$tasks.Count-eq0-and$result.fixture_preserved){'pass'}else{'failed runtime or package-removal gate'}
 }catch{$result.status='failed native MSIX lifecycle';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
 finally{Save 'RESULT' $result}
 exit $(if($result.status-eq'pass'){0}else{1})
}
if($Mode-eq'AfterRemoval'){
 $removed=@{status='running';administrator=$admin;sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value};Save 'AFTER_REMOVAL' $removed
 try{
  if($admin-or$removed.sid-ne$owner.sid){throw 'COM removal observer must use original ordinary user'}
  if(@(Get-AppxPackage -Name 'Ottrin.Development').Count){throw 'Package remains registered'}
  $previous=Read-State (Join-Path $Root ('results\'+$(if($owner.package.previous){'UPDATED'}else{'ACTOR'})+'.json'))
  if(-not$previous.folder_command.status.StartsWith('pass')){throw 'Same-user positive COM activation control required'}
  Initialize-ShellProbe
  if([PackagedShellQA]::HostIdentity()-ne15700){throw 'Removal probe must have no package identity'}
  $code=0;try{$null=[PackagedShellQA]::Title()}catch{$code=$_.Exception.GetBaseException().HResult}
  if($code-ne-2147221164){throw ('Removed COM class must be unregistered; actual HRESULT='+$code)}
  $removed.activation_hresult=$code;$removed.status='pass'
 }catch{$removed.status='failed ordinary COM unregistration';$removed.error=$_.Exception.Message}
 finally{Save 'AFTER_REMOVAL' $removed}
 exit $(if($removed.status-eq'pass'){0}else{1})
}

if($Mode-eq'Login'){
$result=@{status='running';administrator=$admin;cli_exits=@{};scope='Observe Windows-started package daemon before any Ottrin CLI, after actual reboot/login'}
 try{
  if($admin){throw 'Login observer must have an ordinary token'}
  $pending=Read-State (Join-Path $Root 'results\RESULT.json')
  $result.boot_ticks=(Get-CimInstance Win32_OperatingSystem).LastBootUpTime.ToUniversalTime().Ticks
  if($pending.status-ne'awaiting actual login'-or$result.boot_ticks-eq$pending.before_login_boot_ticks){throw 'Login observer requires the next real boot'}
  $package=Get-AppxPackage -Name 'Ottrin.Development';$exe=Join-Path $package.InstallLocation 'bin\ottrin.exe';$daemonExe=Join-Path $package.InstallLocation 'bin\ottrin-indexd.exe'
  if($owner.package.startup_enabled){
  $deadline=[DateTime]::UtcNow.AddMinutes(3)
  do{
   $daemons=@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue|Where-Object {$_.Path-ieq$daemonExe})
   if($daemons.Count){break};Start-Sleep -Milliseconds 250
  }while([DateTime]::UtcNow-lt$deadline)
  if($daemons.Count-ne1){throw 'Windows did not start exactly one package daemon at login'}
  $daemon=$daemons[0];$processOwner=Invoke-CimMethod -InputObject (Get-CimInstance Win32_Process -Filter ('ProcessId='+$daemon.Id)) -MethodName GetOwnerSid
  if($processOwner.Sid-ne$owner.sid-or[InputDesktopQA]::Elevated($daemon.Handle)){throw 'Login daemon token mismatch'}
  [uint32]$length=0;$code=[PackageIdentityQA]::GetPackageFullName($daemon.Handle,[ref]$length,$null);if($code-ne122){throw 'Login daemon package identity unavailable'}
  $buffer=[Text.StringBuilder]::new([int]$length)
  if([PackageIdentityQA]::GetPackageFullName($daemon.Handle,[ref]$length,$buffer)-ne0-or$buffer.ToString()-ne$package.PackageFullName){throw 'Login daemon package identity mismatch'}
  $result.observed_before_any_ottrin_cli=@{pid=$daemon.Id;package=$buffer.ToString();administrator=$false;session=$daemon.SessionId}
  }else{
   $started=[DateTime]::UtcNow;$deadline=$started.AddMinutes(3)
   do{
    if(@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue|Where-Object {$_.Path-ieq$daemonExe}).Count){throw 'Disabled startup launched a package daemon at login'}
    Start-Sleep -Milliseconds 250
   }while([DateTime]::UtcNow-lt$deadline)
   $result.observed_before_any_ottrin_cli=@{daemon_count=0;observation_seconds=([DateTime]::UtcNow-$started).TotalSeconds;startup_enabled=$false}
  }
  $config=Join-Path $env:APPDATA 'ottrin\config.json';$result.preferences_preserved=(Get-FileHash $config).Hash-eq$pending.actor.preferences_before_removal
  if(-not$result.preferences_preserved){throw 'Preferences changed across login'}
  Assert-OwnStartup 'login-'
  if($owner.package.startup_enabled){$diagnostics=Wait-OwnIndexReady 'login-';$result.index_ready=$diagnostics.status}else{
   if(@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue|Where-Object {$_.Path-ieq$daemonExe}).Count){throw 'Passive startup inspection started a disabled daemon'}
   $result.index_state='stopped as configured'
  }
  $result.store=Get-OwnStore
  if($result.store.identity-ne$pending.actor.store.identity-or$result.store.path-ne$pending.actor.store.path){throw 'Login replaced or relocated the saved index database'}
  $result.status='pass'
 }catch{$result.status='failed actual login';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
 finally{Save 'LOGIN' $result}
 exit $(if($result.status-eq'pass'){0}else{1})
}
if($Mode-eq'Updated'){
 $result=@{status='running';administrator=$admin;cli_exits=@{};scope='Actual ordinary post-update GUI, unchanged startup/configuration and declared startup choice after main window closes'};Save 'UPDATED' $result
 try{
  if($admin){throw 'Post-update observer must have an ordinary token'}
  $before=Read-State (Join-Path $Root 'results\BEFORE_UPDATE.json')
  $config=Join-Path $env:APPDATA 'ottrin\config.json';$result.preferences_before_removal=(Get-FileHash $config).Hash
  if($result.preferences_before_removal-ne$before.actor.preferences_before_removal){throw 'Preferences changed during package update'}
  $package=Get-AppxPackage -Name 'Ottrin.Development';$exe=Join-Path $package.InstallLocation 'bin\ottrin.exe'
  if([string]$package.Version-ne$owner.package.version){throw 'Wrong updated package version'}
  Assert-OwnStartup 'updated-'
  $result.store=Get-OwnStore
  if($result.store.identity-ne$before.actor.store.identity-or$result.store.path-ne$before.actor.store.path){throw 'Update replaced or relocated the saved index database'}
  Exercise-MainWindow -StartupCheckbox
  if($result.gui_focus_and_capture-ne'pass'-or-not$result.ContainsKey('normal_close_exit')-or$result.normal_close_exit-ne0){throw 'Updated GUI focus or normal close failed'}
  $daemonExe=Join-Path $package.InstallLocation 'bin\ottrin-indexd.exe'
  $sessionDaemons=@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue|Where-Object {$_.Path-ieq$daemonExe})
  if($owner.package.startup_enabled){
   if($sessionDaemons.Count-ne1){throw 'New background daemon did not survive normal main-window close'}
   $diagnostics=Wait-OwnIndexReady 'updated-';$result.index_ready=$diagnostics.status
  }else{
   $result.session_daemons_after_gui_close=$sessionDaemons.Count
   if($sessionDaemons.Count-gt1){throw 'Multiple session daemons after normal close'}
   if($sessionDaemons.Count-eq1){$diagnostics=Wait-OwnIndexReady 'updated-session-';$result.index_ready=$diagnostics.status}else{
    $process=Invoke-OwnCLI @('index','status','--json') 'updated-cached-index.json' 'updated-cached-index.stderr'
    if($process.ExitCode-ne0){throw 'Read-only saved-index inspection failed'}
    $diagnostics=Read-State (Join-Path $Root 'results\updated-cached-index.json')
    if($diagnostics.indexed_items-ne1){throw 'Disabled update lost the saved one-file index'}
    if(@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue|Where-Object {$_.Path-ieq$daemonExe}).Count){throw 'Saved-index inspection started a disabled session daemon'}
    $result.cached_index_items=$diagnostics.indexed_items
   }
  }
  Assert-OwnStartup 'updated-after-gui-'
  if((Get-FileHash $config).Hash-ne$result.preferences_before_removal){throw 'Post-update GUI altered saved preferences'}
  $result.status='pass'
 }catch{$result.status='failed post-update runtime';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
 finally{Save 'UPDATED' $result}
 exit $(if($result.status-eq'pass'){0}else{1})
}
$result=@{status='running';administrator=$admin;session_id=(Get-Process -Id $PID).SessionId;scope='Actual packaged ordinary main-window launch, normal close and optional index registration; no upgrade acceptance yet'};Save 'ACTOR' $result
$result.cli_exits=@{}
try {
 if($admin){throw 'Ordinary application token required'}
 $fixture=Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'OttrinMSIXFixture';New-Item -ItemType Directory $fixture|Out-Null;[IO.File]::WriteAllText((Join-Path $fixture 'preserve.txt'),"Preserve owned MSIX fixture`n")
 $config=Join-Path $env:APPDATA 'ottrin\config.json';New-Item -ItemType Directory (Split-Path $config) -Force|Out-Null
 [IO.File]::WriteAllText($config,(@{theme='Dark';window_size=@(960,640);default_view_mode='Grid';list_columns=@('Name','Kind','Size','Modified');command_frame_always_visible=$false;show_hidden_files=$false;target=@{current=$null};bookmarks=@();installed_office_previews_enabled=$false;search=@{include_roots=@($fixture);content_indexing_enabled=$false}}|ConvertTo-Json -Depth 5),[Text.UTF8Encoding]::new($false))
 $package=Get-AppxPackage -Name 'Ottrin.Development';$exe=Join-Path $package.InstallLocation 'bin\ottrin.exe'
 Exercise-MainWindow
 $process=Invoke-OwnCLI @('index','service-install') 'service-install.stdout' 'service-install.stderr'
 $result.service_install_exit=$process.ExitCode;if($process.ExitCode-ne0){throw 'Packaged background index setup failed; inspect exact CLI output'}
 $process=Invoke-OwnCLI @('index','service-status') 'service-status.stdout' 'service-status.stderr'
 $result.service_status_exit=$process.ExitCode;if($process.ExitCode-ne0){throw 'Packaged startup inspection failed'}
 $status=Get-Content (Join-Path $Root 'results\service-status.stdout') -Raw
 if($status-notmatch'Windows Startup apps'-or$status-notmatch'Enabled at login'){throw 'Actual package startup must report enabled Windows Startup apps'}
 Exercise-MainWindow -StartupCheckbox
 if($result.msix_startup_checkbox.status-ne'pass actual installed-package checkbox'){throw 'Actual MSIX startup checkbox acceptance missing'}
 $daemonExe=Join-Path $package.InstallLocation 'bin\ottrin-indexd.exe'
 $daemons=@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue|Where-Object {$_.Path-ieq$daemonExe})
 if($daemons.Count-ne1){throw 'Exactly one package index daemon required'}
 $daemon=$daemons[0];[uint32]$length=0;$code=[PackageIdentityQA]::GetPackageFullName($daemon.Handle,[ref]$length,$null)
 if($code-ne122){throw 'Actual daemon has no package identity'}
 $buffer=[Text.StringBuilder]::new([int]$length)
 if([PackageIdentityQA]::GetPackageFullName($daemon.Handle,[ref]$length,$buffer)-ne0-or$buffer.ToString()-ne$package.PackageFullName){throw 'Daemon package identity mismatch'}
 if([InputDesktopQA]::Elevated($daemon.Handle)){throw 'Index daemon must not be elevated'}
 $result.daemon=@{pid=$daemon.Id;package=$buffer.ToString();administrator=$false}
 $diagnostics=Wait-OwnIndexReady ''
 $result.index_ready=$diagnostics.status

 $process=Invoke-OwnCLI @('index','service-remove') 'service-remove.stdout' 'service-remove.stderr'
 $result.service_remove_exit=$process.ExitCode;if($process.ExitCode-ne0){throw 'Packaged disable startup failed'}
 $process=Invoke-OwnCLI @('index','service-status') 'disabled-status.stdout' 'disabled-status.stderr'
 if($process.ExitCode-ne0-or(Get-Content (Join-Path $Root 'results\disabled-status.stdout') -Raw)-notmatch'enabled=false'){throw 'Disabled package startup not reflected in status'}
 if(@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue|Where-Object {$_.Path-ieq$daemonExe}).Count-ne0){throw 'Disabling package indexing did not stop its daemon'}
 $process=Invoke-OwnCLI @('index','service-install') 'reenable.stdout' 'reenable.stderr'
 if($process.ExitCode-ne0){throw 'Package startup could not be enabled again'}
 $diagnostics=Wait-OwnIndexReady 'reenabled-'
 $result.store=Get-OwnStore
 if(-not$owner.package.startup_enabled){
  $process=Invoke-OwnCLI @('index','service-remove') 'chosen-disable.stdout' 'chosen-disable.stderr'
  if($process.ExitCode-ne0){throw 'Unable to choose disabled startup before update'}
  Assert-OwnStartup 'chosen-disabled-'
  if(@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue|Where-Object {$_.Path-ieq$daemonExe}).Count){throw 'Chosen disabled startup did not stop its existing daemon'}
 }
 $result.disable_reenable='pass';$result.preferences_before_removal=(Get-FileHash $config).Hash
 $result.status=if($result.gui_focus_and_capture-eq'pass'-and$result.ContainsKey('normal_close_exit')-and$result.normal_close_exit-eq0){'pass'}else{'partial runtime; GUI focus or close gate failed'}
}catch{$result.status='failed ordinary runtime';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally{Save 'ACTOR' $result}
exit $(if($result.status-eq'pass'){0}else{1})
