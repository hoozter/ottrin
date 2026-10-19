$ErrorActionPreference='Stop';$result=@{status='starting';dispatch_id='86fcf6c760a9cc4b522c797395cd2f9f';binary_sha256='78c8ae7bf6192ed242cffa40e44a3723d3546b1ff29fd4183e09120c3b2db090'};$process=$null;$root='C:\OttrinCopyCleanupQA\power-86fcf6c760a9cc4b522c797395cd2f9f'
try {
 $id=[Security.Principal.WindowsIdentity]::GetCurrent()
 if($env:USERNAME-ne'OttrinCopyQA'-or([Security.Principal.WindowsPrincipal]::new($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Wrong or elevated actor'}
 $result.ordinary_user=$true
 if((Get-FileHash C:\OttrinCopyCleanupQA\power-platform.exe).Hash.ToLowerInvariant()-ne'78c8ae7bf6192ed242cffa40e44a3723d3546b1ff29fd4183e09120c3b2db090'){throw 'Power executable hash mismatch'}
 if(Test-Path $root){throw 'Power fixture exists'}
 New-Item -ItemType Directory $root|Out-Null
 $acl=[Security.AccessControl.DirectorySecurity]::new();$acl.SetSecurityDescriptorSddlForm('D:P(A;OICI;FA;;;OW)(A;OICI;FA;;;SY)',[Security.AccessControl.AccessControlSections]::Access);Set-Acl $root $acl
 New-Item -ItemType Directory "$root\compiler"|Out-Null;$env:TEMP="$root\compiler";$env:TMP=$env:TEMP
 . C:\OttrinCopyCleanupQA\power-sync.ps1
 foreach($name in @('source','source\nested','destination','central')){New-Item -ItemType Directory (Join-Path $root $name)|Out-Null}
 $private=[Security.AccessControl.DirectorySecurity]::new();$private.SetSecurityDescriptorSddlForm('D:P(A;OICI;FA;;;OW)',[Security.AccessControl.AccessControlSections]::Access);Set-Acl "$root\central" $private
 Write-Durable "$root\guard" ([Text.Encoding]::ASCII.GetBytes('86fcf6c760a9cc4b522c797395cd2f9f'))
 Write-Durable "$root\source\nested\file" ([Text.Encoding]::ASCII.GetBytes('original data'))
 Write-Durable "$root\destination\user-data" ([Text.Encoding]::ASCII.GetBytes('unrelated public file'))
 foreach($path in @("$root\source\nested","$root\source","$root\destination","$root\central",$root,'C:\OttrinCopyCleanupQA')){[PowerSync]::Directory($path)}
 $env:OTTRIN_TEST_CLAIM_PUBLICATION_ROOT=$root
 $fixture='copy_transaction::windows::journal::tests::claim_publication::windows_native_copy_temporary_claim_hard_kill_removes_only_its_preparation';$barrier="$root\ready"
 if('Prepared'-eq'Persisted'){$env:OTTRIN_TEST_CLAIM_PUBLICATION_CASE='1';Write-Durable "$root\publish" ([Text.Encoding]::ASCII.GetBytes('go'));$fixture='copy_transaction::windows::journal::tests::claim_publication::windows_native_copy_prepared_claim_publishers_race_without_overwrite';$barrier="$root\result-1"}
 $process=Start-Process C:\OttrinCopyCleanupQA\power-platform.exe -ArgumentList "--exact $fixture --nocapture --test-threads=1" -WorkingDirectory C:\Windows\System32 -PassThru -RedirectStandardOutput C:\OttrinCopyCleanupQA\results\power-before.stdout -RedirectStandardError C:\OttrinCopyCleanupQA\results\power-before.stderr
 $null=$process.Handle;$deadline=(Get-Date).AddSeconds(30)
 while(-not(Test-Path $barrier)){if($process.HasExited-or(Get-Date)-gt$deadline){throw 'Preparation barrier not reached'};Start-Sleep -Milliseconds 50}
 $claims="$root\destination\.ottrin-copy-target-claims"
 $foreign=Join-Path $claims ('.preparing-'+('f'*32));Write-Durable $foreign ([Text.Encoding]::ASCII.GetBytes('foreign preparation'));[PowerSync]::Directory($claims)
 $prep=Join-Path $claims ('.preparing-'+('d'*32));$key="$claims\short-names.ottrin-claims\copy.ottrin-claim"
 if('Prepared'-eq'Prepared'){$result.preparation_sha256=Read-SharedHash $prep;$result.preparation_length=(Get-Item $prep).Length}else{if((Get-Content $barrier -Raw)-ne'owner'){throw 'Claim not owned'};$result.canonical_sha256=Read-SharedHash $key;if(Test-Path (Join-Path $claims ('.preparing-'+('1'*32)))){throw 'Unexpected private preparation'}}
 $result.source_sha256=Read-SharedHash "$root\source\nested\file";$result.public_sha256=Read-SharedHash "$root\destination\user-data";$result.foreign_sha256=Read-SharedHash $foreign
 $result.canonical_exists=Test-Path $key;$result.root=$root;$result.child_pid=$process.Id;$result.phase='Prepared';$result.status='ready-for-cut'
 if(($result.canonical_exists-ne('Prepared'-eq'Persisted'))-or$process.HasExited){throw 'Invalid live preparation state'}
 $result|ConvertTo-Json|Set-Content C:\OttrinCopyCleanupQA\results\POWER_READY.json.tmp
 Move-Item C:\OttrinCopyCleanupQA\results\POWER_READY.json.tmp C:\OttrinCopyCleanupQA\results\POWER_READY.json -Force
 if(-not$process.WaitForExit(120000)){throw 'Host did not cut this fixture'}
 throw 'Preparation exited without the planned VM cut'
} catch {$result.status='fail';$result.error=$_.Exception.Message}
finally {
 if($process-and-not$process.HasExited){& C:\Windows\System32\taskkill.exe /PID $process.Id /T /F|Out-Null;$null=$process.WaitForExit(10000)}
 $result|ConvertTo-Json|Set-Content C:\OttrinCopyCleanupQA\results\POWER_EARLY_EXIT.json
}
exit 1
