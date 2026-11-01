param(
    [string[]]$Executable = @('target/release/ottrin-app.exe', 'target/release/ottrin-indexd.exe'),
    [string]$Icon = 'assets/icon.ico',
    [string]$Output
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class OttrinNativeIcon {
    [DllImport("shell32.dll", EntryPoint="ExtractIconExW", CharSet=CharSet.Unicode)]
    private static extern uint CountIcons(string path, int index, IntPtr large, IntPtr small, uint count);
    [DllImport("shell32.dll", EntryPoint="ExtractIconExW", CharSet=CharSet.Unicode)]
    public static extern uint Extract(string path, int index, out IntPtr large, out IntPtr small, uint count);
    [DllImport("user32.dll")]
    public static extern bool DestroyIcon(IntPtr icon);
    public static uint Count(string path) { return CountIcons(path, -1, IntPtr.Zero, IntPtr.Zero, 0); }
}
'@
function Get-IconPixels([IntPtr]$Handle) {
    $icon = $null; $bitmap = $null; $stream = $null; $writer = $null; $hash = $null
    try {
        $icon = [Drawing.Icon]::FromHandle($Handle)
        $bitmap = $icon.ToBitmap()
        $stream = [IO.MemoryStream]::new()
        $writer = [IO.BinaryWriter]::new($stream)
        $writer.Write([int]$bitmap.Width); $writer.Write([int]$bitmap.Height)
        for ($y=0; $y-lt$bitmap.Height; $y++) {
            for ($x=0; $x-lt$bitmap.Width; $x++) { $writer.Write($bitmap.GetPixel($x,$y).ToArgb()) }
        }
        $writer.Flush()
        $hash = [Security.Cryptography.SHA256]::Create()
        return @{ width=$bitmap.Width; height=$bitmap.Height; sha256=[BitConverter]::ToString($hash.ComputeHash($stream.ToArray())).Replace('-','').ToLowerInvariant() }
    } finally {
        if ($hash) { $hash.Dispose() }; if ($writer) { $writer.Dispose() }; if ($stream) { $stream.Dispose() }
        if ($bitmap) { $bitmap.Dispose() }; if ($icon) { $icon.Dispose() }
        # FromHandle does not own the native HICON; the caller releases it.
    }
}
function Get-NativeIcon([string]$Path) {
    $path = (Resolve-Path -LiteralPath $Path).ProviderPath
    $count = [OttrinNativeIcon]::Count($path)
    if ($count-eq0 -or $count-eq[uint32]::MaxValue) { throw "No embedded icon group: $path" }
    $large = [IntPtr]::Zero; $small = [IntPtr]::Zero
    try {
        $read = [OttrinNativeIcon]::Extract($path,0,[ref]$large,[ref]$small,1)
        if ($read-eq0 -or $read-eq[uint32]::MaxValue -or $large-eq[IntPtr]::Zero -or $small-eq[IntPtr]::Zero) { throw "Windows could not extract both icon sizes: $path (returned=$read, large=$large, small=$small)" }
        return @{ path=$path; groups=$count; extracted=$read; large=(Get-IconPixels $large); small=(Get-IconPixels $small) }
    } finally {
        $released = $true
        foreach ($handle in @($large,$small)) {
            if ($handle-ne[IntPtr]::Zero -and -not[OttrinNativeIcon]::DestroyIcon($handle)) { $released=$false }
        }
        if (-not$released) { throw 'Windows could not release an extracted test icon' }
    }
}
$expected = Get-NativeIcon $Icon
$results = @()
foreach ($path in $Executable) {
    $actual = Get-NativeIcon $path
    foreach ($size in @('large','small')) {
        if ($actual[$size].sha256-ne$expected[$size].sha256) { throw "Executable icon differs from the canonical asset ($size): $path" }
    }
    $results += $actual
}
$json = @{ status='pass'; method='Native Windows Shell extraction, dimensions and ARGB pixels'; canonical=$expected; executables=$results } | ConvertTo-Json -Depth 8
if ($Output) { [IO.File]::WriteAllText([IO.Path]::GetFullPath($Output),$json,[Text.UTF8Encoding]::new($false)) }
Write-Output $json
