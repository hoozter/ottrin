$ErrorActionPreference='Stop';$result=@{status='starting';dispatch_id='5d659c3043dbc71105546618652f9b04';binary_sha256='5df963d265f8f4b06736976f961a0572b11e8f01bba944b90a74eafa364a565e'};$process=$null;$root='C:\OttrinCopyCleanupQA\power-5d659c3043dbc71105546618652f9b04'
try {
 $id=[Security.Principal.WindowsIdentity]::GetCurrent()
 if($env:USERNAME-ne'OttrinCopyQA'-or([Security.Principal.WindowsPrincipal]::new($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Wrong or elevated actor'}
 $result.ordinary_user=$true
 if((Get-FileHash C:\OttrinCopyCleanupQA\power-platform.exe).Hash.ToLowerInvariant()-ne'5df963d265f8f4b06736976f961a0572b11e8f01bba944b90a74eafa364a565e'){throw 'Power executable hash mismatch'}
 if(Test-Path $root){throw 'Power fixture exists'}
 New-Item -ItemType Directory $root|Out-Null
 $acl=[Security.AccessControl.DirectorySecurity]::new();$acl.SetSecurityDescriptorSddlForm('D:P(A;OICI;FA;;;OW)(A;OICI;FA;;;SY)',[Security.AccessControl.AccessControlSections]::Access);Set-Acl $root $acl
 New-Item -ItemType Directory "$root\compiler"|Out-Null;$env:TEMP="$root\compiler";$env:TMP=$env:TEMP
 . C:\OttrinCopyCleanupQA\power-sync.ps1
 foreach($name in @('source','source\nested','destination','central')){New-Item -ItemType Directory (Join-Path $root $name)|Out-Null}
 $private=[Security.AccessControl.DirectorySecurity]::new();$private.SetSecurityDescriptorSddlForm('D:P(A;OICI;FA;;;OW)',[Security.AccessControl.AccessControlSections]::Access);Set-Acl "$root\central" $private
 Write-Durable "$root\guard" ([Text.Encoding]::ASCII.GetBytes('5d659c3043dbc71105546618652f9b04'))
 Write-Durable "$root\source\nested\file" ([Text.Encoding]::ASCII.GetBytes('original data'))
 Write-Durable "$root\destination\user-data" ([Text.Encoding]::ASCII.GetBytes('unrelated public file'))
 foreach($path in @("$root\source\nested","$root\source","$root\destination","$root\central",$root,'C:\OttrinCopyCleanupQA')){[PowerSync]::Directory($path)}
 $env:OTTRIN_TEST_CLAIM_PUBLICATION_ROOT=$root
 $process=Start-Process C:\OttrinCopyCleanupQA\power-platform.exe -ArgumentList '--exact copy_transaction::windows::journal::tests::claim_publication::windows_native_copy_temporary_claim_hard_kill_removes_only_its_preparation --nocapture --test-threads=1' -WorkingDirectory C:\Windows\System32 -PassThru -RedirectStandardOutput C:\OttrinCopyCleanupQA\results\power-before.stdout -RedirectStandardError C:\OttrinCopyCleanupQA\results\power-before.stderr
 $null=$process.Handle;$deadline=(Get-Date).AddSeconds(30)
 while(-not(Test-Path "$root\ready")){if($process.HasExited-or(Get-Date)-gt$deadline){throw 'Preparation barrier not reached'};Start-Sleep -Milliseconds 50}
 $claims="$root\destination\.ottrin-copy-target-claims"
 $foreign=Join-Path $claims ('.preparing-'+('f'*32));Write-Durable $foreign ([Text.Encoding]::ASCII.GetBytes('foreign preparation'));[PowerSync]::Directory($claims)
 $prep=Join-Path $claims ('.preparing-'+('d'*32));$key="$claims\short-names.ottrin-claims\copy.ottrin-claim"
 $result.preparation_sha256=Read-SharedHash $prep;$result.preparation_length=(Get-Item $prep).Length
 $result.source_sha256=Read-SharedHash "$root\source\nested\file";$result.public_sha256=Read-SharedHash "$root\destination\user-data";$result.foreign_sha256=Read-SharedHash $foreign
 $result.canonical_exists=Test-Path $key;$result.root=$root;$result.child_pid=$process.Id;$result.phase='Prepared';$result.status='ready-for-cut'
 if($result.canonical_exists-or$process.HasExited){throw 'Invalid live preparation state'}
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
