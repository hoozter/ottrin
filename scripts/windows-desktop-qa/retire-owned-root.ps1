$ErrorActionPreference='Stop'
$root='C:\OttrinWindowQA'
$receipt=Get-Content "$root\cleanup-result.json" -Raw|ConvertFrom-Json
if($receipt.status-ne'pass'-or-not$receipt.profile_removed-or-not$receipt.login_values_restored-or-not$receipt.temporary_password_removed){throw 'Restoration must finish before fixture retirement'}
if(Get-LocalUser OttrinWindowQA -ErrorAction SilentlyContinue){throw 'Owned account remains'}
if(Get-ScheduledTask 'Ottrin Window Input QA' -ErrorAction SilentlyContinue){throw 'Owned actor remains'}
if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like"$root\*"}).Count){throw 'Owned executable remains'}
function GrantFixtureAccess([string]$path){
 $entry=Get-Item -LiteralPath $path -Force
 # Grant/delete one entry at a time: never recurse through a junction or link.
 if($entry.Attributes-band[IO.FileAttributes]::ReparsePoint){throw 'Unexpected reparse point in owned fixture'}
 & "$env:windir\system32\takeown.exe" /f $path /a|Out-Null
 if($LASTEXITCODE-ne0){throw 'Owned fixture ownership restoration failed'}
 & "$env:windir\system32\icacls.exe" $path /grant '*S-1-5-18:F'|Out-Null
 if($LASTEXITCODE-ne0){throw 'Owned fixture access restoration failed'}
 if($entry.PSIsContainer){foreach($child in Get-ChildItem -LiteralPath $path -Force){GrantFixtureAccess $child.FullName}}
}
# Only this disposable tree is retired after the account and credential are gone.
# Retained production data outside this exact QA root is never touched.
GrantFixtureAccess $root
Remove-Item -LiteralPath $root -Recurse -Force
if(Test-Path $root){throw 'Owned fixture root remains'}
Write-Output 'Owned fixtures removed, including protected transaction data.'
