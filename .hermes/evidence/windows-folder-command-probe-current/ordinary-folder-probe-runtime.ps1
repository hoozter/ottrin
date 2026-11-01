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
 public class Result {public bool started;public int error,exit;}
 [DllImport("shell32.dll",CharSet=CharSet.Unicode,SetLastError=true)]static extern bool ShellExecuteExW(ref Info value);
 [DllImport("kernel32.dll")]static extern uint WaitForSingleObject(IntPtr process,uint millis);
 [DllImport("kernel32.dll")]static extern bool GetExitCodeProcess(IntPtr process,out uint code);
 [DllImport("kernel32.dll")]static extern bool CloseHandle(IntPtr handle);
 [DllImport("kernel32.dll")]static extern bool TerminateProcess(IntPtr handle,uint code);
 public static Result Run(string verb,string directory) {var i=new Info();i.size=(uint)Marshal.SizeOf(i);i.mask=0x40|0x100|0x400;i.verb=verb;i.file=directory;i.show=0;var r=new Result();r.started=ShellExecuteExW(ref i);r.error=Marshal.GetLastWin32Error();if(!r.started)return r;if(i.process==IntPtr.Zero)throw new Exception("Missing owned process handle");try{if(WaitForSingleObject(i.process,10000)!=0){TerminateProcess(i.process,1);WaitForSingleObject(i.process,10000);throw new Exception("Owned help process timeout");}uint code;if(!GetExitCodeProcess(i.process,out code))throw new Exception("Missing exit code");r.exit=(int)code;return r;}finally{CloseHandle(i.process);}}
}
'@
$cases=@();$registered=@{};$base=[Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Software\Classes\Directory\shell')
try {
 $corpus=Join-Path $homeRoot 'target folder';New-Item -ItemType Directory $corpus|Out-Null
 foreach($name in @('plain','literal%1','literal%V','100% literal')) {
  $dir=Join-Path $homeRoot $name;New-Item -ItemType Directory $dir|Out-Null;$exe=Join-Path $dir 'ottrin.exe';New-Item -ItemType HardLink -Path $exe -Target "$root\fixture-launch.exe"|Out-Null
  foreach($encoding in @('raw','doubled')) {
   $verb='Ottrin.QA.'+[guid]::NewGuid().ToString('N');if($base.OpenSubKey($verb)){throw 'Foreign fixture key retained'}
   $path=if($encoding-eq'raw'){$exe}else{$exe.Replace('%','%%')};$command='"'+$path+'" --help -- "%1"';$registered[$verb]=$command
   $key=$base.CreateSubKey($verb);try{$key.SetValue('','Owned Ottrin command probe');$child=$key.CreateSubKey('command');try{$child.SetValue('',$command);$child.Flush()}finally{$child.Dispose()};$key.Flush()}finally{$key.Dispose()}
   $result=[FolderShell]::Run($verb,$corpus);$cases+=@{directory=$name;encoding=$encoding;command=$command;started=$result.started;error=$result.error;exit=$result.exit}
   if($name-eq'plain'-and$encoding-eq'raw'-and(-not$result.started-or$result.exit-ne0)){throw 'Normal-path shell control failed'}
  }
 }
 @{status='observed';scope='Actual ShellExecuteEx of private ordinary-token Directory verbs, source executable Help only; no default association changes';cases=$cases}|ConvertTo-Json -Depth 8|Set-Content "$root\results\$phase.json"
} catch {($_|Out-String)+$_.ScriptStackTrace|Set-Content "$root\results\$phase-error.txt";throw} finally {
 foreach($verb in @($registered.Keys)) {
  $key=$base.OpenSubKey($verb);if(-not$key){continue}
  try {if(($key.GetValueNames()-join',')-ne''-or($key.GetSubKeyNames()-join',')-ne'command'-or$key.GetValue('')-ne'Owned Ottrin command probe'){throw 'Modified fixture verb retained'};$child=$key.OpenSubKey('command');try{if(($child.GetValueNames()-join',')-ne''-or$child.SubKeyCount-ne0-or$child.GetValue('')-ne$registered[$verb]){throw 'Modified fixture command retained'}}finally{$child.Dispose()}}finally{$key.Dispose()}
  $base.DeleteSubKeyTree($verb,$false)
 }
 $base.Dispose();Remove-Item -LiteralPath $homeRoot -Recurse -Force
}
