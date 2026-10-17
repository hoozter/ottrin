$ErrorActionPreference='Stop'
$root='C:\OttrinCopyCleanupQA\office-native'
$process=$null;$report=@{status='running'}
try {
 if($env:USERNAME-ne'campbell'){throw 'Wrong native actor'}
 $id=[Security.Principal.WindowsIdentity]::GetCurrent();if(([Security.Principal.WindowsPrincipal]::new($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Elevated actor'}
 if([Diagnostics.Process]::GetCurrentProcess().SessionId-le0){throw 'Interactive session required'}
 $report.interactive=$true;$report.effective_administrator=$false;$report.filtered_administrator_membership=@($id.Groups|Where-Object{$_.Value-eq'S-1-5-32-544'}).Count-gt0
 $env:TEMP="$root\temp";$env:TMP=$env:TEMP
 if(Test-Path $env:TEMP){throw 'Native temporary root already exists'}
 New-Item -ItemType Directory $env:TEMP|Out-Null
 $env:OTTRIN_OFFICE_HELPER="$root\ottrin-office-render.exe";$env:OTTRIN_NATIVE_OFFICE_FIXTURES="$root\corpus";$env:OTTRIN_PDFIUM_DIR=$root;$env:PATH="$root;$env:PATH"
 $process=Start-Process "$root\native.exe" -ArgumentList '--exact native_installed_office_renders_each_advertised_format native_installed_office_converts_and_cleans_private_state native_installed_office_cancels_after_provider_start_and_releases_files --ignored --nocapture --test-threads=1' -WorkingDirectory $root -PassThru -RedirectStandardOutput "$root\native.stdout" -RedirectStandardError "$root\native.stderr"
 $null=$process.Handle
 if(-not$process.WaitForExit(420000)){throw 'Native format sweep exceeded owned deadline'}
 $report.exit=$process.ExitCode;if($process.ExitCode-ne0){throw 'Native contained format sweep failed'}
 $report.temporary_entries=@(Get-ChildItem $env:TEMP -Force).Count;if($report.temporary_entries){throw 'Native profile or output leak'}
 $report.status='pass'
} catch {$report.status='fail';$report.error=$_.Exception.ToString()}
finally {
 if($process-and-not$process.HasExited){& C:\Windows\System32\taskkill.exe /PID $process.Id /T /F|Out-Null;$null=$process.WaitForExit(10000)}
 $report.owned_processes=@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like"$root\*"-or$_.ExecutablePath-like"$root\temp\*"}).Count
 if($report.owned_processes){$report.status='fail'}else{if(Test-Path "$root\temp"){Remove-Item -LiteralPath "$root\temp" -Recurse -Force};$report.temporary_root_removed=(-not(Test-Path "$root\temp"))}
 $report|ConvertTo-Json|Set-Content "$root\RESULTS.json"
}
if($report.status-ne'pass'){exit 1}
