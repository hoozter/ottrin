$ErrorActionPreference='Stop'
$root='C:\OttrinOrdinaryQA'
$name='ottrin-ordinary-qa'
$description='Ottrin ordinary QA a230fa2c disposable'
$password='aA!5'+[Guid]::NewGuid().ToString('N')+[Guid]::NewGuid().ToString('N')
$user=Get-LocalUser -Name $name -ErrorAction SilentlyContinue
if($user){
 $owner=Get-Content "$root\OWNER.json" -Raw|ConvertFrom-Json
 if($user.Description-ne$description-or$user.SID.Value-ne$owner.sid){throw 'Existing account is not owned'}
 if(Test-Path "$root\CONTROLLER.json"){throw 'Actor already dispatched; observe existing result'}
 Set-LocalUser -Name $name -Password (ConvertTo-SecureString $password -AsPlainText -Force)
}else{
 $user=New-LocalUser -Name $name -Password (ConvertTo-SecureString $password -AsPlainText -Force) -Description $description -AccountNeverExpires
}
if(-not(Get-LocalGroupMember -Group (Get-LocalGroup -SID 'S-1-5-32-545')|Where-Object {$_.SID-eq$user.SID})){Add-LocalGroupMember -Group (Get-LocalGroup -SID 'S-1-5-32-545') -Member $user}
@{sid=$user.SID.Value;name=$name;description=$description;profileLoaded=$false}|ConvertTo-Json|Set-Content "$root\OWNER.json"
$acl=Get-Acl $root
$acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($user.SID,'ReadAndExecute','ContainerInherit,ObjectInherit','None','Allow'));Set-Acl $root $acl
foreach($child in @('data','results')){
 $path=Join-Path $root $child
 if(-not(Test-Path $path)){New-Item -ItemType Directory $path|Out-Null}
 $a=Get-Acl $path
 $a.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($user.SID,'FullControl','ContainerInherit,ObjectInherit','None','Allow'));Set-Acl -LiteralPath $path -AclObject $a
}
Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices; using System.ComponentModel;
public static class OrdinaryNative {
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] public struct Startup {public int cb; public string reserved,desktop,title; public int x,y,cx,cy,xchars,ychars,fill,flags; public short show,cbReserved; public IntPtr reserved2,input,output,error;}
 [StructLayout(LayoutKind.Sequential)] public struct ProcessInfo {public IntPtr process,thread; public uint pid,tid;}
 [DllImport("advapi32.dll",SetLastError=true,CharSet=CharSet.Unicode)] static extern bool LogonUser(string name,string domain,string password,int type,int provider,out IntPtr token);
 [DllImport("advapi32.dll",SetLastError=true,CharSet=CharSet.Unicode)] static extern bool CreateProcessAsUser(IntPtr token,string application,string command,IntPtr pa,IntPtr ta,bool inherit,uint flags,IntPtr environment,string directory,ref Startup startup,out ProcessInfo process);
 [DllImport("kernel32.dll",SetLastError=true)] static extern uint WaitForSingleObject(IntPtr handle,uint milliseconds);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetExitCodeProcess(IntPtr process,out uint exit);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool TerminateProcess(IntPtr process,uint code);
 [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
 public static uint Run(string name,string password,string root) {
  IntPtr token=IntPtr.Zero;ProcessInfo p=new ProcessInfo();
  try {
   if(!LogonUser(name,".",password,2,0,out token))throw new Win32Exception(Marshal.GetLastWin32Error(),"LogonUser");
   var s=new Startup();s.cb=Marshal.SizeOf(s);
   string exe=Environment.GetFolderPath(Environment.SpecialFolder.Windows)+"\\System32\\WindowsPowerShell\\v1.0\\powershell.exe";
   string command="\""+exe+"\" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File \""+root+"\\actor.ps1\"";
   if(!CreateProcessAsUser(token,exe,command,IntPtr.Zero,IntPtr.Zero,false,0x08000000,IntPtr.Zero,root,ref s,out p))throw new Win32Exception(Marshal.GetLastWin32Error(),"CreateProcessAsUser");
   if(WaitForSingleObject(p.process,300000)!=0){TerminateProcess(p.process,1);WaitForSingleObject(p.process,10000);throw new Exception("Owned actor timed out; process terminated");}
   uint code;if(!GetExitCodeProcess(p.process,out code))throw new Win32Exception(Marshal.GetLastWin32Error());return code;
  } finally {if(p.thread!=IntPtr.Zero)CloseHandle(p.thread);if(p.process!=IntPtr.Zero)CloseHandle(p.process);if(token!=IntPtr.Zero)CloseHandle(token);}
 }
}
'@
$exit=[OrdinaryNative]::Run($name,$password,$root)
$password=$null
@{actorExit=$exit;profileLoadingRequested=$false;method='LogonUser interactive token and CreateProcessAsUser CREATE_NO_WINDOW; session 0; no scheduled task or logon settings changed'}|ConvertTo-Json|Set-Content "$root\CONTROLLER.json"
if($exit-ne0){throw "Owned actor failed with exit $exit; retain results for diagnosis"}
