$ErrorActionPreference='Stop';$root='C:\OttrinJobFixtureQA';$phase=(Get-Content "$root\phase" -Raw).Trim()
Start-Transcript -Path "$root\results\$phase.transcript"|Out-Null
$identity=[Security.Principal.WindowsIdentity]::GetCurrent();$owner=Get-Content "$root\INTERACTIVE_OWNER.json" -Raw|ConvertFrom-Json
if($identity.User.Value-ne$owner.sid-or([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Expected ordinary interactive owner'}
$homeRoot=Join-Path $root "data\$phase";New-Item -ItemType Directory $homeRoot|Out-Null
foreach($pair in @(@('TEMP','t'),@('TMP','t'),@('APPDATA','a'),@('LOCALAPPDATA','l'),@('USERPROFILE','h'),@('HOME','h'))){$path=Join-Path $homeRoot $pair[1];New-Item -ItemType Directory $path -Force|Out-Null;[Environment]::SetEnvironmentVariable($pair[0],$path,'Process')}
Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;
public static class FolderShell {
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)]public struct Info {public uint size,mask;public IntPtr hwnd;public string verb,file,parameters,directory;public int show;public IntPtr instance,idList;public string klass;public IntPtr key;public uint hotkey;public IntPtr icon,process;}
 public class Result {public bool started,owned;public int error,exit=-999;public string image;public uint pid;}
 [DllImport("shell32.dll",CharSet=CharSet.Unicode,SetLastError=true)]static extern bool ShellExecuteExW(ref Info value);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)]static extern bool QueryFullProcessImageNameW(IntPtr p,uint flags,System.Text.StringBuilder name,ref uint count);
 [DllImport("kernel32.dll")]static extern uint GetProcessId(IntPtr p);
 [DllImport("kernel32.dll")]static extern uint WaitForSingleObject(IntPtr process,uint millis);
 [DllImport("kernel32.dll")]static extern bool GetExitCodeProcess(IntPtr process,out uint code);
 [DllImport("kernel32.dll")]static extern bool CloseHandle(IntPtr handle);
 [DllImport("kernel32.dll")]static extern bool TerminateProcess(IntPtr handle,uint code);
 public static Result Run(string verb,string directory,IntPtr klass) {var i=new Info();i.size=(uint)Marshal.SizeOf(i);i.mask=0x40|0x100|0x400|3;i.key=klass;i.verb=verb;i.file=directory;i.show=0;var r=new Result();r.started=ShellExecuteExW(ref i);r.error=Marshal.GetLastWin32Error();if(!r.started)return r;if(i.process==IntPtr.Zero)return r;try{r.pid=GetProcessId(i.process);var name=new System.Text.StringBuilder(32768);uint count=(uint)name.Capacity;if(!QueryFullProcessImageNameW(i.process,0,name,ref count))throw new Exception("Cannot identify returned process");r.image=name.ToString();if(!String.Equals(r.image,System.IO.Path.Combine(Environment.SystemDirectory,"cmd.exe"),StringComparison.OrdinalIgnoreCase))return r;r.owned=true;if(WaitForSingleObject(i.process,10000)!=0){TerminateProcess(i.process,1);WaitForSingleObject(i.process,10000);throw new Exception("Owned help process timeout");}uint code;if(!GetExitCodeProcess(i.process,out code))throw new Exception("Missing exit code");r.exit=(int)code;return r;}finally{CloseHandle(i.process);}}
}
'@

$cases=@();$registered=@{};$nonce=(Get-Content "$root\OWNER.json" -Raw|ConvertFrom-Json).nonce;$anchor='Software\Ottrin.CommandProbe.'+$nonce.Replace('-','')
$manifest=@{nonce=$nonce;anchor=$anchor;commands=@{}}
$manifest|ConvertTo-Json -Depth 8|Set-Content "$root\results\$phase-registry-owner.json"
if([Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($anchor)){throw 'Existing private class retained'}
$class=[Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($anchor,[Microsoft.Win32.RegistryKeyPermissionCheck]::ReadWriteSubTree,[Microsoft.Win32.RegistryOptions]::Volatile)
$class.SetValue('',('Owned Ottrin command probe '+$nonce))
$base=$class.CreateSubKey('shell',[Microsoft.Win32.RegistryKeyPermissionCheck]::ReadWriteSubTree,[Microsoft.Win32.RegistryOptions]::Volatile)
try {
 $corpus=Join-Path $homeRoot 'target folder';New-Item -ItemType Directory $corpus|Out-Null
 foreach($item in @(@('open',61),@('Ottrin.Probe',62))) {
  $verb=$item[0];$command='"'+[Environment]::SystemDirectory+'\cmd.exe" /d /c exit '+$item[1];$registered[$verb]=$command;$manifest.commands[$verb]=$command
  $manifest|ConvertTo-Json -Depth 8|Set-Content "$root\results\$phase-registry-owner.json"
  $key=$base.CreateSubKey($verb,[Microsoft.Win32.RegistryKeyPermissionCheck]::ReadWriteSubTree,[Microsoft.Win32.RegistryOptions]::Volatile)
  try {$key.SetValue('','Owned Ottrin command probe');$child=$key.CreateSubKey('command',[Microsoft.Win32.RegistryKeyPermissionCheck]::ReadWriteSubTree,[Microsoft.Win32.RegistryOptions]::Volatile);try{$child.SetValue('',$command);$child.Flush()}finally{$child.Dispose()};$key.Flush()}finally{$key.Dispose()}
 }
 foreach($test in @(@{default=$null;verb=$null},@{default='open';verb=$null},@{default='Ottrin.Probe';verb=$null},@{default='Ottrin.Probe';verb='open'})) {
  if($null-eq$test.default){$base.DeleteValue('',$false)}else{$base.SetValue('',$test.default,[Microsoft.Win32.RegistryValueKind]::String)};$base.Flush()
  $result=[FolderShell]::Run($test.verb,$corpus,$class.Handle.DangerousGetHandle());$cases+=@{default=$test.default;requestedVerb=$test.verb;started=$result.started;owned=$result.owned;image=$result.image;pid=$result.pid;error=$result.error;exit=$result.exit}
  if(-not$result.started-or-not$result.owned){throw 'Private class did not launch the controlled cmd.exe verb'}
 }
 @{status='observed';scope='Actual Windows ShellExecuteEx private volatile class with open and alternate verbs; default selection and explicit-open bypass; no personal default association changes';cases=$cases}|ConvertTo-Json -Depth 8|Set-Content "$root\results\$phase.json"
} catch {($_|Out-String)+$_.ScriptStackTrace|Set-Content "$root\results\$phase-error.txt";throw} finally {
 $base.DeleteValue('',$false)
 foreach($verb in @($registered.Keys)) {
  $key=$base.OpenSubKey($verb);if(-not$key){continue}
  try {if(($key.GetValueNames()-join',')-ne''-or($key.GetSubKeyNames()-join',')-ne'command'-or$key.GetValue('')-ne'Owned Ottrin command probe'){throw 'Modified fixture verb retained'};$child=$key.OpenSubKey('command');try{if(($child.GetValueNames()-join',')-ne''-or$child.SubKeyCount-ne0-or$child.GetValue('')-ne$registered[$verb]){throw 'Modified fixture command retained'}}finally{$child.Dispose()}}finally{$key.Dispose()}
  $base.DeleteSubKeyTree($verb,$false)
 }
 $base.Dispose();if(($class.GetValueNames()-join',')-ne''-or$class.GetValue('')-ne('Owned Ottrin command probe '+$nonce)-or($class.GetSubKeyNames()-join',')-ne'shell'){throw 'Changed private class retained'};$shell=$class.OpenSubKey('shell');try{if($shell.SubKeyCount-ne0-or$shell.ValueCount-ne0){throw 'Changed class shell retained'}}finally{$shell.Dispose()};$class.Dispose();[Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree($anchor,$false)
 $manifest.removed=$true;$manifest|ConvertTo-Json -Depth 8|Set-Content "$root\results\$phase-registry-owner.json"
 Remove-Item -LiteralPath $homeRoot -Recurse -Force
}
