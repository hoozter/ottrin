$ErrorActionPreference='Stop';$result=@{status='running';scope='Explicit SYSTEM audit fixture'};$p=$null;$temp='C:\OttrinContentQA\audit-temp'
try {
 $id=[Security.Principal.WindowsIdentity]::GetCurrent();if($id.User.Value-ne'S-1-5-18'){throw 'Wrong audit fixture principal'}
 if(Test-Path $temp){throw 'Owned audit namespace exists'};New-Item -ItemType Directory $temp|Out-Null;$env:TEMP=$temp;$env:TMP=$temp
 $p=Start-Process C:\OttrinContentQA\platform.exe -ArgumentList '--exact copy_transaction::windows::security::tests::new_copy_candidate_inherits_the_real_parent_audit_sacl --ignored --nocapture --test-threads=1' -PassThru -RedirectStandardOutput C:\OttrinContentQA\results\audit.stdout -RedirectStandardError C:\OttrinContentQA\results\audit.stderr
 $null=$p.Handle;if(-not$p.WaitForExit(60000)){throw "Audit fixture remains live: $($p.Id)"};$result.exit=$p.ExitCode
 $result.temporary_entries=@(Get-ChildItem $temp -Force).Count
 $result.status='collected'
} catch {$result.status='fail';$result.error=$_.Exception.Message}
finally {
 if($p-and-not$p.HasExited){& C:\Windows\System32\taskkill.exe /PID $p.Id /T /F|Out-Null;$null=$p.WaitForExit(10000)}
 if(Test-Path $temp){Remove-Item $temp -Recurse -Force}
 $result|ConvertTo-Json|Set-Content C:\OttrinContentQA\results\AUDIT_RESULTS.json
}
if($result.status-ne'collected'){exit 1}
