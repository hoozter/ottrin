$ErrorActionPreference='Stop'
$root='C:\OttrinCopyCleanupQA\office-native'
$process=$null;$report=@{status='running'}
try {
 if($env:USERNAME-ne'OttrinCopyQA'){throw 'Wrong native actor'}
 $id=[Security.Principal.WindowsIdentity]::GetCurrent();if(([Security.Principal.WindowsPrincipal]::new($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Elevated actor'}
 if([Diagnostics.Process]::GetCurrentProcess().SessionId-ne0){throw 'Expected batch session'}
 $report.noninteractive_session_id=0
 $env:TEMP="$root\temp";$env:TMP=$env:TEMP
 if(Test-Path $env:TEMP){throw 'Native temporary root already exists'}
 New-Item -ItemType Directory $env:TEMP|Out-Null
 $env:OTTRIN_OFFICE_HELPER="$root\ottrin-office-render.exe";$env:OTTRIN_NATIVE_OFFICE_FIXTURES="$root\corpus";$env:OTTRIN_PDFIUM_DIR=$root;$env:PATH="$root;$env:PATH"
 $process=Start-Process "$root\native.exe" -ArgumentList '--exact windows_native_office_metadata_probe_is_desktop_independent --ignored --nocapture --test-threads=1' -WorkingDirectory $root -PassThru -RedirectStandardOutput "$root\metadata.stdout" -RedirectStandardError "$root\metadata.stderr"
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
 $report|ConvertTo-Json|Set-Content "$root\METADATA_RESULTS.json"
}
if($report.status-ne'pass'){exit 1}
