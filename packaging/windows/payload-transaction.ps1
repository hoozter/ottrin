param(
    [Parameter(Mandatory=$true)]
    [ValidateSet('Initialize','Publish','BeginIntegration','Rollback','FinishRollback','SealCommit','Commit')][string]$Phase,
    [string]$Target=$env:OTTRIN_INSTALLER_TARGET
)
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
if (-not [IO.Path]::IsPathRooted($Target) -or [IO.Path]::GetPathRoot($Target) -eq '\' -or -not [IO.Path]::GetPathRoot($Target).EndsWith('\')) { throw 'Installation path must be absolute.' }
$Target=[IO.Path]::GetFullPath($Target).TrimEnd('\')
if ($Target -eq [IO.Path]::GetPathRoot($Target).TrimEnd('\')) { throw 'A volume root cannot be an installation directory.' }
$Root="$Target.ottrin-setup"
$New=Join-Path $Root 'new'
$Old=Join-Path $Root 'previous'
$Journal=Join-Path $Root 'transaction.json'
$User=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value

# NSIS runs children from its plugin directory, which contains a native
# System.dll. The C# compiler otherwise mistakes that plugin for its framework
# reference. Use this process's trusted CLR directory before compiling the
# small identity binding; never search the payload for framework assemblies.
$framework=[Runtime.InteropServices.RuntimeEnvironment]::GetRuntimeDirectory()
[Environment]::CurrentDirectory=$framework
Set-Location -LiteralPath $framework

# A retained directory is never identified by its pathname alone. Open the link
# itself; refuse reparse points before walking or moving its contents.
Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
public static class OttrinInstallIdentity {
 [StructLayout(LayoutKind.Sequential)] struct Info {
  public uint Attr; public System.Runtime.InteropServices.ComTypes.FILETIME Created,Accessed,Written;
  public uint Volume,SizeHigh,SizeLow,Links,IndexHigh,IndexLow;
 }
 [StructLayout(LayoutKind.Sequential)] struct FileId {public ulong Volume,Low,High;}
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetFileInformationByHandleEx(SafeFileHandle handle,int kind,out FileId id,uint size);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)]
 static extern SafeFileHandle CreateFile(string path,uint access,uint share,IntPtr security,uint disposition,uint flags,IntPtr template);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetFileInformationByHandle(SafeFileHandle handle,out Info info);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool CreateDirectory(string path,IntPtr security);
 public static void Create(string path) {if(!CreateDirectory(path,IntPtr.Zero))throw new Win32Exception(Marshal.GetLastWin32Error());}
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern uint GetFinalPathNameByHandle(SafeFileHandle h,System.Text.StringBuilder p,uint n,uint flags);
 [DllImport("kernel32.dll",SetLastError=true)] static extern bool SetFileInformationByHandle(SafeFileHandle h,int kind,ref byte value,uint length);
 static string Identify(SafeFileHandle h) {
  Info i;if(!GetFileInformationByHandle(h,out i))throw new Win32Exception(Marshal.GetLastWin32Error());
  if((i.Attr&0x400)!=0)throw new InvalidOperationException("Reparse entry is not an installer payload.");
  var path=new System.Text.StringBuilder(32768);uint n=GetFinalPathNameByHandle(h,path,(uint)path.Capacity,1);
  if(n==0 || n>=path.Capacity)throw new Win32Exception(Marshal.GetLastWin32Error());
  string name=path.ToString();int end=name.IndexOf('}');
  if(!name.StartsWith(@"\\?\Volume{",StringComparison.Ordinal) || end<0)throw new InvalidOperationException("A local volume identity is required.");
  FileId id;if(!GetFileInformationByHandleEx(h,18,out id,24))throw new Win32Exception(Marshal.GetLastWin32Error());
  return name.Substring(0,end+1)+":"+id.Volume.ToString("x16")+":"+id.High.ToString("x16")+id.Low.ToString("x16");
 }
 public static bool Retire(string path,string identity,string hash) {
  // Deny writers and rename/delete holders while checking and disposing this
  // exact object. The disposition applies to this handle, never a fresh name.
  using(var h=CreateFile(path,0x80010000,1,IntPtr.Zero,3,0x00200000,IntPtr.Zero)) {
   if(h.IsInvalid)throw new Win32Exception(Marshal.GetLastWin32Error());
   if(Identify(h)!=identity)return false;
   using(var stream=new System.IO.FileStream(h,System.IO.FileAccess.Read)) {
    using(var sha=System.Security.Cryptography.SHA256.Create()) {
     if(BitConverter.ToString(sha.ComputeHash(stream)).Replace("-","")!=hash)return false;
    }
    if(Identify(h)!=identity)return false;
    byte delete=1;if(!SetFileInformationByHandle(h,4,ref delete,1))throw new Win32Exception(Marshal.GetLastWin32Error());
   }
   return true;
  }
 }
 public static void WriteShortcut(string path,byte[] bytes) {
  using(var h=CreateFile(path,0x40000000,1,IntPtr.Zero,4,0x00200000,IntPtr.Zero)) {
   if(h.IsInvalid)throw new Win32Exception(Marshal.GetLastWin32Error());
   Identify(h);
   using(var stream=new System.IO.FileStream(h,System.IO.FileAccess.Write)) {
    stream.SetLength(0);stream.Write(bytes,0,bytes.Length);stream.Flush(true);
   }
  }
 }
 public static string Read(string path) {
  using(var h=CreateFile(path,0,7,IntPtr.Zero,3,0x02200000,IntPtr.Zero)) {
   if(h.IsInvalid)throw new Win32Exception(Marshal.GetLastWin32Error());
   return Identify(h);
  }
 }
}
'@
function Identity($path) { [OttrinInstallIdentity]::Read($path) }
function Expect($path,$identity) { if ((Identity $path) -cne $identity) { throw "Installation identity changed: $path" } }
function Save($record) {
    $bytes=[Text.UTF8Encoding]::new($false).GetBytes(($record|ConvertTo-Json -Depth 12 -Compress))
    $temporary=Join-Path $Root ('journal-'+[Guid]::NewGuid().ToString('N'))
    $file=[IO.File]::Open($temporary,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
    try {$file.Write($bytes,0,$bytes.Length);$file.Flush($true)} finally {$file.Dispose()}
    if ([IO.File]::Exists($Journal)) {[IO.File]::Replace($temporary,$Journal,[NullString]::Value)} else {[IO.File]::Move($temporary,$Journal)}
}
function Relative($path,$base) {
    $prefix=$base+'\'
    if (-not $path.StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase)) { throw 'Payload entry is outside its installation.' }
    $path.Substring($prefix.Length)
}
function Member($base,$relative) {
    $path=[IO.Path]::GetFullPath((Join-Path $base $relative))
    $null=Relative $path $base
    return $path
}
function Files($path) {
    $null=Identity $path
    foreach($entry in [IO.Directory]::EnumerateFileSystemEntries($path)) {
        $null=Identity $entry
        if ([IO.Directory]::Exists($entry)) { Files $entry } else { $entry }
    }
}
function Stamp($path) {
    @{identity=(Identity $path);sha256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash}
}
function Retire($path,$stamp) {
    [OttrinInstallIdentity]::Retire($path,$stamp.identity,$stamp.sha256)
}
function EmptyDirectories($path) {
    $null=Identity $path
    foreach($entry in [IO.Directory]::EnumerateDirectories($path)) { EmptyDirectories $entry }
    if (@([IO.Directory]::EnumerateFileSystemEntries($path)).Count -eq 0) { [IO.Directory]::Delete($path) }
}
$RegistrySpec=@(
    @{path='Software\Ottrin';names=@('InstallDir')},
    @{path='Software\Microsoft\Windows\CurrentVersion\Uninstall\Ottrin';names=@('DisplayName','DisplayVersion','Publisher','DisplayIcon','UninstallString','NoModify','NoRepair')}
)
function RegistrySnapshot($spec) {
    $key=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($spec.path)
    try {
        $values=@(foreach($name in $spec.names) {
            $exists=$null -ne $key -and $key.GetValueNames() -contains $name
            if($exists) {
                $kind=$key.GetValueKind($name)
                if($kind -eq [Microsoft.Win32.RegistryValueKind]::Unknown){throw 'Unsupported registry value kind.'}
                @{name=$name;exists=$true;kind=[int]$kind;value=$key.GetValue($name,$null,[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)}
            } else {@{name=$name;exists=$false}}
        })
        @{path=$spec.path;exists=($null -ne $key);values=$values}
    } finally {if($null -ne $key){$key.Dispose()}}
}
function RestoreRegistry($saved) {
    $key=[Microsoft.Win32.Registry]::CurrentUser.CreateSubKey($saved.path)
    try {
        foreach($v in $saved.values) {
            if(-not $v.exists){$key.DeleteValue($v.name,$false);continue}
            $kind=[Microsoft.Win32.RegistryValueKind][int]$v.kind
            $value=switch($kind) {
                Binary { ,[byte[]]$v.value }
                None { ,[byte[]]$v.value }
                MultiString { ,[string[]]$v.value }
                DWord { [int]$v.value }
                QWord { [long]$v.value }
                default { [string]$v.value }
            }
            $key.SetValue($v.name,$value,$kind)
        }
        $empty=$key.GetValueNames().Count -eq 0 -and $key.GetSubKeyNames().Count -eq 0
    } finally {$key.Dispose()}
    if(-not $saved.exists -and $empty){[Microsoft.Win32.Registry]::CurrentUser.DeleteSubKey($saved.path,$false)}
}
function Finish($record) {
    # Only the exact candidate files are disposable. Unexpected or edited files
    # stay available outside the live installation and do not block later setup.
    if ([IO.Directory]::Exists($New)) {
        if($null -ne $record.candidate){Expect $New $record.candidate}
        $null=Files $New | Out-Null
        $owned=$record.files
        if($null -eq $record.candidate) {
            $owned=@(foreach($path in (Files $New)){@{path=(Relative $path $New);candidate=(Stamp $path)}})
        }
        foreach($entry in $owned) {
            $path=Member $New $entry.path
            if([IO.File]::Exists($path)){$null=Retire $path $entry.candidate}
        }
        EmptyDirectories $New
    }
    if([IO.Directory]::Exists($Old) -or [IO.Directory]::Exists($New)) {
        $retained=$Target+'.ottrin-retained-'+[Guid]::NewGuid().ToString('N')
        if($env:OTTRIN_INSTALLER_PAYLOAD_NOTICE) {
            [IO.File]::WriteAllText($env:OTTRIN_INSTALLER_PAYLOAD_NOTICE,$retained,[Text.Encoding]::Unicode)
        }
        [IO.Directory]::Move($Root,$retained)
        Write-Output "Changed or unowned previous files remain at $retained"
    } else {
        Expect $Root $record.root
        [IO.File]::Delete($Journal)
        [IO.Directory]::Delete($Root)
    }
}
$Menu=Join-Path ([Environment]::GetFolderPath('Programs')) 'Ottrin'
$Links=@('Ottrin.lnk','Uninstall Ottrin.lnk')
if ($Phase -eq 'Initialize') {
    if (Test-Path -LiteralPath $Root) {throw "A previous installation transaction is retained at $Root. Its files have not been changed."}
    $parent=[IO.Path]::GetDirectoryName($Target)
    $null=[IO.Directory]::CreateDirectory($parent)
    $null=Identity $parent
    $record=@{version=1;user=$User;target=$Target;root=$null;state='staging';previous=$null;candidate=$null;files=@();registry=@();links=@()}
    if (Test-Path -LiteralPath $Target) {
        if (-not [IO.Directory]::Exists($Target)) {throw 'Installation target is not a directory.'}
        $null=Files $Target | Out-Null
        if(@([IO.Directory]::EnumerateFileSystemEntries($Target)).Count -ne 0 -and
            (-not [IO.File]::Exists((Join-Path $Target 'bin\ottrin.exe')) -or -not [IO.File]::Exists((Join-Path $Target 'Uninstall.exe')))) {
            throw 'The nonempty target is not an Ottrin NSIS installation. Choose an empty directory or uninstall the other package first.'
        }
        $record.previous=Identity $Target
    }
    $record.registry=@(foreach($spec in $RegistrySpec) {RegistrySnapshot $spec})
    $record.links=@(foreach($name in $Links) {
        $path=Join-Path $Menu $name
        if (Test-Path -LiteralPath $path) {
            $null=Identity $path
            @{name=$name;bytes=[Convert]::ToBase64String([IO.File]::ReadAllBytes($path))}
        } else {@{name=$name;bytes=$null}}
    })
    [OttrinInstallIdentity]::Create($Root)
    $record.root=Identity $Root
    try {
        $null=[IO.Directory]::CreateDirectory($New)
        Save $record
    } catch {
        if([IO.Directory]::Exists($New) -and @([IO.Directory]::EnumerateFileSystemEntries($New)).Count -eq 0){[IO.Directory]::Delete($New)}
        if(@([IO.Directory]::EnumerateFileSystemEntries($Root)).Count -eq 0){[IO.Directory]::Delete($Root)}
        throw
    }
    return
}
$record=Get-Content -LiteralPath $Journal -Raw -Encoding UTF8|ConvertFrom-Json
if ($record.version -ne 1 -or $record.user -cne $User -or $record.target -cne $Target) {throw 'Installation transaction identity is invalid.'}
Expect $Root $record.root
if (@($record.registry).Count -ne 2 -or @($record.links).Count -ne 2) {throw 'Installation integration snapshot is incomplete.'}
for($i=0;$i -lt 2;$i++) {
    $spec=$RegistrySpec[$i]
    if ($record.registry[$i].path -cne $spec.path -or $record.links[$i].name -cne $Links[$i]) {throw 'Installation snapshot is outside its integration scope.'}
    if(@($record.registry[$i].values).Count -ne $spec.names.Count){throw 'Registry snapshot is incomplete.'}
    for($j=0;$j -lt $spec.names.Count;$j++){if($record.registry[$i].values[$j].name -cne $spec.names[$j]){throw 'Registry snapshot value is outside its integration scope.'}}
}
if ($Phase -eq 'Publish') {
    if ($record.state -ne 'staging') {throw 'Installation is not ready for publication.'}
    $record.candidate=Identity $New
    $record.files=@(foreach($path in (Files $New)) {
        $relative=Relative $path $New
        $prior=$null
        if ($null -ne $record.previous) {
            Expect $Target $record.previous
            $oldPath=Member $Target $relative
            if (Test-Path -LiteralPath $oldPath) {
                if ([IO.Directory]::Exists($oldPath)) {throw 'A package file would replace an existing directory.'}
                $prior=Stamp $oldPath
            }
        }
        @{path=$relative;candidate=(Stamp $path);previous=$prior}
    })
    $record.state='publishing';Save $record
    if ($null -ne $record.previous) {Expect $Target $record.previous;[IO.Directory]::Move($Target,$Old);Expect $Old $record.previous}
    Expect $New $record.candidate
    [IO.Directory]::Move($New,$Target)
    Expect $Target $record.candidate
    $record.state='published';Save $record
} elseif ($Phase -eq 'BeginIntegration') {
    if($record.state -ne 'published'){throw 'Payload publication has not completed.'}
    Expect $Target $record.candidate
    for($i=0;$i -lt $RegistrySpec.Count;$i++) {
        $current=RegistrySnapshot $RegistrySpec[$i]
        $saved=$record.registry[$i]
        for($j=0;$j -lt $RegistrySpec[$i].names.Count;$j++) {
            $a=$current['values'][$j];$b=$saved.values[$j]
            if($a.exists -ne $b.exists -or ($a.exists -and ($a.kind -ne $b.kind -or (ConvertTo-Json -Compress -InputObject $a.value) -cne (ConvertTo-Json -Compress -InputObject $b.value)))) {throw 'Installation registration changed before setup could update it.'}
        }
    }
    foreach($link in $record.links) {
        $path=Join-Path $Menu $link.name
        $bytes=$null
        if(Test-Path -LiteralPath $path){$null=Identity $path;$bytes=[Convert]::ToBase64String([IO.File]::ReadAllBytes($path))}
        if($bytes -cne $link.bytes){throw 'Installation shortcut changed before setup could update it.'}
    }
    $record.state='integrating';Save $record
} elseif ($Phase -eq 'Rollback') {
    if ($record.state -eq 'committed') {throw 'A committed installation cannot be rolled back.'}
    if ($null -ne $record.candidate -and [IO.Directory]::Exists($Target) -and (Identity $Target) -ceq $record.candidate) {
        if (Test-Path -LiteralPath $New) {throw 'Candidate retention name is occupied.'}
        [IO.Directory]::Move($Target,$New)
    }
    if ([IO.Directory]::Exists($Old)) {
        Expect $Old $record.previous
        if (Test-Path -LiteralPath $Target) {throw 'The original installation path is occupied.'}
        [IO.Directory]::Move($Old,$Target)
    }
    if ($null -ne $record.previous) {Expect $Target $record.previous}
    if($record.state -eq 'integrating'){
        foreach($key in $record.registry) {RestoreRegistry $key}
        foreach($link in $record.links) {
            $path=Join-Path $Menu $link.name
            if ($null -eq $link.bytes) {if(Test-Path -LiteralPath $path){[IO.File]::Delete($path)}}
            else {$null=[IO.Directory]::CreateDirectory($Menu);[OttrinInstallIdentity]::WriteShortcut($path,[Convert]::FromBase64String($link.bytes))}
        }
        if ([IO.Directory]::Exists($Menu) -and @([IO.Directory]::EnumerateFileSystemEntries($Menu)).Count -eq 0) {[IO.Directory]::Delete($Menu)}
    }
    $record.state='rolled-back';Save $record
} elseif ($Phase -eq 'FinishRollback') {
    if($record.state -ne 'rolled-back'){throw 'Payload rollback has not completed.'}
    Finish $record
} elseif ($Phase -eq 'SealCommit') {
    if ($record.state -ne 'integrating') {throw 'Only an integrated installation can commit.'}
    Expect $Target $record.candidate
    # Persist the commit boundary before reclaiming anything from the old tree.
    $record.state='committed';Save $record
} else {
    if ($record.state -ne 'committed') {throw 'The installation commit decision has not been recorded.'}
    Expect $Target $record.candidate
    if ($null -ne $record.previous -and (Test-Path -LiteralPath $Old)) {
        Expect $Old $record.previous
        $null=Files $Old | Out-Null # Refuse reparse entries before deleting any old file.
        foreach($entry in $record.files) {
            if ($null -ne $entry.previous) {
                $path=Member $Old $entry.path
                # A committed cleanup may resume after earlier leaves were retired.
                if ((Test-Path -LiteralPath $path) -and -not (Retire $path $entry.previous)) {throw "Previous payload changed and is retained: $path"}
            }
        }
        EmptyDirectories $Old
    }
    Finish $record
}
