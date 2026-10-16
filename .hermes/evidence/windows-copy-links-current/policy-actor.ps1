$ErrorActionPreference='Stop';$result=@{status='running'};$p=$null;$temp=$null
try {
 if($env:USERNAME-ne'OttrinContentQA'){throw 'Wrong test account'}
 $identity=[Security.Principal.WindowsIdentity]::GetCurrent();if(([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Elevated test account'}
 $result.ordinary_user=$true;$temp=Join-Path $env:TEMP 'op-WindowsCopy';if(Test-Path $temp){throw 'Owned namespace exists'};New-Item -ItemType Directory $temp|Out-Null;$env:TEMP=$temp;$env:TMP=$temp
 $p=Start-Process C:\OttrinContentQA\platform.exe -ArgumentList '--exact copy_transaction::windows::links::tests::windows_native_copy_symlink_policy_denial_retains_private_candidate --ignored --nocapture --test-threads=1' -WorkingDirectory C:\Windows\System32 -PassThru -RedirectStandardOutput C:\OttrinContentQA\results\platform.stdout -RedirectStandardError C:\OttrinContentQA\results\platform.stderr
 $null=$p.Handle;if(-not$p.WaitForExit(60000)){throw "Owned test remains live: $($p.Id)"}
 $result.exit=$p.ExitCode;if($p.ExitCode-ne0){throw 'Native stream tests failed'}
 $result.temporary_entries=@(Get-ChildItem -LiteralPath $temp -Force).Count;if($result.temporary_entries){throw 'Tests left temporary data'}
 $result.status='pass'
} catch {$result.status='fail';$result.error=$_.Exception.Message}
finally {
 if($p-and-not$p.HasExited){& C:\Windows\System32\taskkill.exe /PID $p.Id /T /F|Out-Null;$null=$p.WaitForExit(10000)}
 if($temp-and(Test-Path $temp)){Remove-Item -LiteralPath $temp -Recurse -Force}
 $result|ConvertTo-Json|Set-Content C:\OttrinContentQA\results\RESULTS.json
}
if($result.status-ne'pass'){exit 1}
