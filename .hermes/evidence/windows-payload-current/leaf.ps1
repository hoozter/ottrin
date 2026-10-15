$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$r='C:\OttrinQA-20261011\payload-current';$work="$r\leaf3"
function Assert($v,$m){if(-not $v){throw $m}}
$source=Get-Content "$r\payload-transaction.qa.ps1" -Raw
$definition=[regex]::Match($source,"(?s)Add-Type -TypeDefinition @'\r?\n(.*?)\r?\n'@").Groups[1].Value
Add-Type -TypeDefinition $definition
Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;using Microsoft.Win32.SafeHandles;
public static class PayloadMap {
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] public static extern IntPtr CreateFileMapping(SafeFileHandle h,IntPtr sa,uint protect,uint high,uint low,string name);
 [DllImport("kernel32.dll",SetLastError=true)] public static extern IntPtr MapViewOfFile(IntPtr h,uint access,uint high,uint low,UIntPtr count);
 [DllImport("kernel32.dll",SetLastError=true)] public static extern bool FlushViewOfFile(IntPtr p,UIntPtr count);
 [DllImport("kernel32.dll")] public static extern bool UnmapViewOfFile(IntPtr p);
 [DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr h);
}
'@
try {
 New-Item -ItemType Directory $work|Out-Null
 $leaf="$work\leaf.txt";[IO.File]::WriteAllText($leaf,'original data')
 $id=[OttrinInstallIdentity]::Read($leaf);$hash=(Get-FileHash $leaf).Hash
 [IO.File]::Move($leaf,"$work\claimed.txt");[IO.File]::WriteAllText($leaf,'replacement data')
 Assert (-not [OttrinInstallIdentity]::Retire($leaf,$id,$hash)) 'Stale identity retired replacement'
 Assert ((Get-Content $leaf -Raw) -ceq 'replacement data') 'Replacement changed'
 Assert (-not [OttrinInstallIdentity]::Retire("$work\claimed.txt",$id,('0'*64))) 'Wrong bytes retired'
 Assert ([OttrinInstallIdentity]::Retire("$work\claimed.txt",$id,$hash)) 'Exact private leaf did not retire'
 Assert (-not (Test-Path "$work\claimed.txt")) 'Exact leaf remains'
 [IO.File]::WriteAllText($leaf,'mapped original')
 $id=[OttrinInstallIdentity]::Read($leaf);$hash=(Get-FileHash $leaf).Hash
 $f=[IO.File]::Open($leaf,[IO.FileMode]::Open,[IO.FileAccess]::ReadWrite,[IO.FileShare]7)
 $map=[PayloadMap]::CreateFileMapping($f.SafeFileHandle,[IntPtr]::Zero,4,0,0,[NullString]::Value)
 Assert ($map -ne [IntPtr]::Zero) 'Could not create writable section'
 $view=[PayloadMap]::MapViewOfFile($map,2,0,0,[UIntPtr]::new([uint64]15))
 Assert ($view -ne [IntPtr]::Zero) 'Could not map writable view'
 $f.Dispose();$f=$null
 try {
  $refused=$false;$detail=''
  try {$retired=[OttrinInstallIdentity]::Retire($leaf,$id,$hash);$refused=-not $retired} catch {$refused=$true;$detail=$_|Out-String}
  Assert $refused 'Closed-handle writable mapping was retired'
  Assert (Test-Path $leaf) 'Mapped file name disappeared'
  [Runtime.InteropServices.Marshal]::WriteByte($view,0,77)
  Assert ([PayloadMap]::FlushViewOfFile($view,[UIntPtr]::new([uint64]15))) 'Flush mapping failed'
  Assert ((Get-Content $leaf -Raw).StartsWith('Mapped')) 'Late mapped write not preserved'
 } finally {[PayloadMap]::UnmapViewOfFile($view)|Out-Null;[PayloadMap]::CloseHandle($map)|Out-Null}
 @{status='pass';stale_identity_refused=$true;changed_bytes_refused=$true;exact_leaf_retired=$true;closed_handle_writable_mapping_refused=$true;late_mapped_write_preserved=$true;mapping_refusal=$detail}|ConvertTo-Json|Set-Content "$r\LEAF.json"
} catch {$_|Out-String|Set-Content "$r\leaf-failure.txt";exit 1}
