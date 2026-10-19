$ErrorActionPreference='Stop';$result=@{status='recovering';dispatch_id='20892ddcdf6eb488bc19efac347465cb';root='C:\OttrinCopyCleanupQA\power-20892ddcdf6eb488bc19efac347465cb';binary_sha256='5df963d265f8f4b06736976f961a0572b11e8f01bba944b90a74eafa364a565e'};$process=$null
try {
 $id=[Security.Principal.WindowsIdentity]::GetCurrent();if($env:USERNAME-ne'OttrinCopyQA'-or([Security.Principal.WindowsPrincipal]::new($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Wrong or elevated recovery actor'}
 $root='C:\OttrinCopyCleanupQA\power-20892ddcdf6eb488bc19efac347465cb';if((Get-Content "$root\guard" -Raw)-ne'20892ddcdf6eb488bc19efac347465cb'){throw 'Fixture ownership changed'}
 if((Get-FileHash C:\OttrinCopyCleanupQA\power-platform.exe).Hash.ToLowerInvariant()-ne'5df963d265f8f4b06736976f961a0572b11e8f01bba944b90a74eafa364a565e'){throw 'Recovery binary changed'}
 $env:TEMP="$root\compiler";$env:TMP=$env:TEMP;. C:\OttrinCopyCleanupQA\power-sync.ps1
 $env:OTTRIN_TEST_CLAIM_PUBLICATION_ROOT=$root;$env:OTTRIN_TEST_CLAIM_PUBLICATION_CASE='1'
 Write-Durable "$root\publish" ([Text.Encoding]::ASCII.GetBytes('go'))
 $process=Start-Process C:\OttrinCopyCleanupQA\power-platform.exe -ArgumentList '--exact copy_transaction::windows::journal::tests::claim_publication::windows_native_copy_prepared_claim_publishers_race_without_overwrite --nocapture --test-threads=1' -WorkingDirectory C:\Windows\System32 -PassThru -RedirectStandardOutput C:\OttrinCopyCleanupQA\results\power-recover.stdout -RedirectStandardError C:\OttrinCopyCleanupQA\results\power-recover.stderr
 $null=$process.Handle;$deadline=(Get-Date).AddSeconds(30)
 while(-not(Test-Path "$root\result-1")){if($process.HasExited-or(Get-Date)-gt$deadline){throw 'Fresh admission did not complete'};Start-Sleep -Milliseconds 50}
 if((Get-Content "$root\result-1" -Raw)-ne'owner'-or$process.HasExited){throw 'Fresh actor did not retain ownership'}
 $claims="$root\destination\.ottrin-copy-target-claims";$prep=Join-Path $claims ('.preparing-'+('d'*32));$foreign=Join-Path $claims ('.preparing-'+('f'*32));$key="$claims\short-names.ottrin-claims\copy.ottrin-claim"
 $result.fresh_admission=$true;$result.canonical_sha256=Read-SharedHash $key;$result.preparation_sha256=Read-SharedHash $prep;$result.foreign_sha256=Read-SharedHash $foreign
 $result.source_sha256=Read-SharedHash "$root\source\nested\file";$result.public_sha256=Read-SharedHash "$root\destination\user-data";$result.ordinary_user=$true
 Write-Durable "$root\exit" ([Text.Encoding]::ASCII.GetBytes('exit'))
 if(-not$process.WaitForExit(10000)-or$process.ExitCode-ne86){throw 'Fresh owner did not exit as expected'}
 $result.owner_exit=$process.ExitCode;$result.status='recovered-name'
} catch {$result.status='fail';$result.error=$_.Exception.Message}
finally {
 if($process-and-not$process.HasExited){& C:\Windows\System32\taskkill.exe /PID $process.Id /T /F|Out-Null;$null=$process.WaitForExit(10000)}
 foreach($name in @('stdout','stderr')){$path='C:\OttrinCopyCleanupQA\results\power-recover.'+$name;if(Test-Path $path){$result[$name+'_sha256']=(Get-FileHash $path).Hash.ToLowerInvariant()}}
 $result|ConvertTo-Json|Set-Content C:\OttrinCopyCleanupQA\results\POWER_RECOVERY.json.tmp
 Move-Item C:\OttrinCopyCleanupQA\results\POWER_RECOVERY.json.tmp C:\OttrinCopyCleanupQA\results\POWER_RECOVERY.json -Force
}
if($result.status-ne'recovered-name'){exit 1}
