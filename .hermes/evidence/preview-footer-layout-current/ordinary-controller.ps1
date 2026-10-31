$ErrorActionPreference='Stop';$root='C:\OttrinJobFixtureQA';$sid=[Security.Principal.SecurityIdentifier]::new('S-1-5-21-3183210791-1673105385-3262580894-1001');$profile=Get-CimInstance Win32_UserProfile|Where-Object {$_.SID-eq$sid.Value};if(-not$profile.Loaded-or$profile.LocalPath-ne'C:\Users\campbell'){throw 'Expected existing loaded guest profile unavailable'};if(Get-LocalUser -Name 'ottrin-job-qa' -ErrorAction SilentlyContinue){throw 'Disposable actor account was not removed'};@{sid=$sid.Value;existingProfile=$true;profileLoadingRequested=$false;session=1}|ConvertTo-Json|Set-Content "$root\INTERACTIVE_OWNER.json";$acl=Get-Acl $root;$acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($sid,'ReadAndExecute','ContainerInherit,ObjectInherit','None','Allow'));Set-Acl $root $acl;foreach($child in @('data','results')){$path=Join-Path $root $child;if(-not(Test-Path $path)){New-Item -ItemType Directory $path|Out-Null};$a=Get-Acl $path;$a.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($sid,'FullControl','ContainerInherit,ObjectInherit','None','Allow'));Set-Acl $path $a};Set-Content "$root\phase" 'short-temp-ordinary-fixed'
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices; using System.ComponentModel;
public static class InteractiveNative {
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] public struct Startup {public int cb; public string reserved,desktop,title; public int x,y,cx,cy,xchars,ychars,fill,flags; public short show,cbReserved; public IntPtr reserved2,input,output,error;}
 [StructLayout(LayoutKind.Sequential)] public struct ProcessInfo {public IntPtr process,thread; public uint pid,tid;}
 [DllImport("advapi32.dll",SetLastError=true,CharSet=CharSet.Unicode)] static extern bool LogonUser(string name,string domain,string password,int type,int provider,out IntPtr token);
 [DllImport("advapi32.dll",SetLastError=true,CharSet=CharSet.Unicode)] static extern bool CreateProcessAsUser(IntPtr token,string application,string command,IntPtr pa,IntPtr ta,bool inherit,uint flags,IntPtr environment,string directory,ref Startup startup,out ProcessInfo process);
 [DllImport("kernel32.dll",SetLastError=true)] static extern uint WaitForSingleObject(IntPtr handle,uint milliseconds);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetExitCodeProcess(IntPtr process,out uint exit);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool TerminateProcess(IntPtr process,uint code);
 [DllImport("wtsapi32.dll",SetLastError=true)] static extern bool WTSQueryUserToken(uint session,out IntPtr token);
 [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
 public static uint Run(string name,string password,string root) {
  IntPtr token=IntPtr.Zero;ProcessInfo p=new ProcessInfo();
  try {
   if(!WTSQueryUserToken(1,out token))throw new Win32Exception(Marshal.GetLastWin32Error(),"LogonUser");
   var s=new Startup();s.cb=Marshal.SizeOf(s);
   string exe=Environment.GetFolderPath(Environment.SpecialFolder.Windows)+"\\System32\\WindowsPowerShell\\v1.0\\powershell.exe";
   string command="\""+exe+"\" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File \""+root+"\\actor.ps1\"";
   if(!CreateProcessAsUser(token,exe,command,IntPtr.Zero,IntPtr.Zero,false,0x08000000,IntPtr.Zero,root,ref s,out p))throw new Win32Exception(Marshal.GetLastWin32Error(),"CreateProcessAsUser");
   if(WaitForSingleObject(p.process,720000)!=0){TerminateProcess(p.process,1);WaitForSingleObject(p.process,10000);throw new Exception("Owned actor timed out; process terminated");}
   uint code;if(!GetExitCodeProcess(p.process,out code))throw new Win32Exception(Marshal.GetLastWin32Error());return code;
  } finally {if(p.thread!=IntPtr.Zero)CloseHandle(p.thread);if(p.process!=IntPtr.Zero)CloseHandle(p.process);if(token!=IntPtr.Zero)CloseHandle(token);}
 }
}
'@

$exit=[InteractiveNative]::Run('unused','unused',$root);@{actorExit=$exit;method='Existing ordinary interactive WTS token; no credentials or profile loading'}|ConvertTo-Json|Set-Content "$root\INTERACTIVE_CONTROLLER.json";if($exit-ne0){throw "Interactive fixture failed $exit"}
