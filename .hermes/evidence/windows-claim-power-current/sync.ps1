Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class PowerSync {
 [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
 static extern IntPtr CreateFile(string path, uint access, uint share, IntPtr security, uint disposition, uint flags, IntPtr template);
 [DllImport("kernel32.dll", SetLastError=true)] static extern bool FlushFileBuffers(IntPtr file);
 [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr file);
 public static void Directory(string path) {
  IntPtr h=CreateFile(path,0xC0000000,7,IntPtr.Zero,3,0x02200000,IntPtr.Zero);
  if(h==new IntPtr(-1)) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error());
  try { if(!FlushFileBuffers(h)) throw new System.ComponentModel.Win32Exception(Marshal.GetLastWin32Error()); }
  finally { CloseHandle(h); }
 }
}
'@
function Write-Durable($path,[byte[]]$bytes) {
 $s=[IO.File]::Open($path,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read)
 try {$s.Write($bytes,0,$bytes.Length);$s.Flush($true)}finally{$s.Dispose()}
}
function Read-SharedHash($path) {
 $s=[IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]7)
 $hash=[Security.Cryptography.SHA256]::Create()
 try {return ([BitConverter]::ToString($hash.ComputeHash($s))).Replace('-','').ToLowerInvariant()}finally{$s.Dispose();$hash.Dispose()}
}
