param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExpectedSid)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$process=$null;$out=Join-Path $Root 'results'
$result=@{status='running';runs=@();scope='Ordinary-client/SYSTEM-companion caller registry creation/reopen/coordinator and native security/registry regressions; no full transaction, alternate-administrator UAC or package acceptance'}
function Save{[IO.File]::WriteAllText((Join-Path $out 'RESULT.json'),($result|ConvertTo-Json -Depth 18),[Text.UTF8Encoding]::new($false))}
function Require($value,$message){if(-not$value){throw $message}}
Save
try {
 $id=[Security.Principal.WindowsIdentity]::GetCurrent()
 Require ($id.User.Value-eq$ExpectedSid) 'Fresh owned actor required'
 Require (-not([Security.Principal.WindowsPrincipal]::new($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Ordinary actor token required'
 $result.identity=@{sid=$ExpectedSid;administrator=$false;session=(Get-Process -Id $PID).SessionId};Require ($result.identity.session-gt0) 'Owned interactive session required'
 $result.phase='waiting for owned desktop';Save;$ready=Join-Path $out 'DESKTOP_READY.json';$deadline=[DateTime]::UtcNow.AddMinutes(8)
 while(-not(Test-Path $ready)){Require ([DateTime]::UtcNow-lt$deadline) 'Owned desktop observation timed out';Start-Sleep -Milliseconds 200}
 Require ((Get-Content $ready -Raw|ConvertFrom-Json).sid-eq$ExpectedSid) 'Desktop readiness belongs to another user'
 $probeInput=Get-Content (Join-Path $Root 'application-input.json') -Raw|ConvertFrom-Json;$exe=Join-Path $Root 'boundary-test.exe'
 Require ((Get-FileHash $exe).Hash.ToLowerInvariant()-eq$probeInput.test_sha256) 'Native executable mismatch';$result.input=$probeInput
 $failedCases=@()
 foreach($case in @(@{name='ordinary cross-principal client';arguments='--exact windows_privileged_transport::tests::caller_owner::client --ignored --nocapture --test-threads=1';expected='test result: ok. 1 passed; 0 failed'},@{name='native Copy security regressions';arguments='copy_transaction::windows::security::tests:: --skip windows_native_copy_final_link_dacl_does_not_change_populated_targets --nocapture --test-threads=1';expected='test result: ok. 11 passed; 0 failed; 1 ignored'},@{name='native registry and namespace regressions';arguments='copy_transaction::windows::registry::tests:: --nocapture --test-threads=1';expected='test result: ok. 9 passed; 0 failed'},@{name='large canonical security-plan codec regression';arguments='--exact copy_transaction::windows::journal::tests::windows_native_copy_streamed_plan_codec_exceeds_old_single_frame_without_native_authority --nocapture --test-threads=1';expected='test result: ok. 1 passed; 0 failed'},@{name='saved ancestry binding and torn cohort checkpoint regression';arguments='--exact copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cohort_torn_checkpoint_replays_only_validated_prefix --nocapture --test-threads=1';expected='test result: ok. 1 passed; 0 failed'})){
 $result.phase=$case.name;Save
 $info=[Diagnostics.ProcessStartInfo]::new($exe,$case.arguments)
 $info.UseShellExecute=$false;$info.CreateNoWindow=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true
 $info.StandardOutputEncoding=[Text.UTF8Encoding]::new($false);$info.StandardErrorEncoding=[Text.UTF8Encoding]::new($false)
 $info.EnvironmentVariables['OTTRIN_CALLER_PROBE_ROOT']=$Root
 $process=[Diagnostics.Process]::Start($info);$null=$process.Handle;$stdout=$process.StandardOutput.ReadToEndAsync();$stderr=$process.StandardError.ReadToEndAsync()
 Require ($process.WaitForExit(180000)) 'Owned ordinary native test timed out';$process.WaitForExit()
 Require ($stdout.Wait(5000)-and$stderr.Wait(5000)) 'Ordinary test streams did not close'
 $result.runs+=@{case=$case.name;exit=$process.ExitCode;stdout=$stdout.Result;stderr=$stderr.Result};Save
 if(-not($process.ExitCode-eq0-and$stdout.Result.Contains($case.expected))){$failedCases+= $case.name}
 $process.Dispose();$process=$null
 }
 Require ($failedCases.Count-eq0) ('Native assertions failed: '+($failedCases-join', '))
 $result.phase='waiting for owned SYSTEM completion';Save;$done=Join-Path $out 'SYSTEM_RESULT.json';$deadline=[DateTime]::UtcNow.AddMinutes(2)
 while(-not(Test-Path $done)){Require ([DateTime]::UtcNow-lt$deadline) 'SYSTEM result missing';Start-Sleep -Milliseconds 200}
 $system=Get-Content $done -Raw|ConvertFrom-Json;$result.system=$system;Require ($system.status.StartsWith('pass')) 'SYSTEM companion failed'
 $result.observation=Get-Content (Join-Path $out 'OWNER_OBSERVATION.json') -Raw|ConvertFrom-Json
 $result.status='pass canonical caller profile registry and native security/registry/codec regressions';$result.phase='complete'
}catch{$result.status='failed';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace;$systemResult=Join-Path $out 'SYSTEM_RESULT.json';if(Test-Path (Join-Path $out 'SYSTEM_PROCESS.json')){$failureDeadline=[DateTime]::UtcNow.AddSeconds(15);while((-not(Test-Path $systemResult))-and[DateTime]::UtcNow-lt$failureDeadline){Start-Sleep -Milliseconds 100}};if(Test-Path $systemResult){try{$result.system=Get-Content $systemResult -Raw|ConvertFrom-Json}catch{$result.system_diagnostic_error=$_.Exception.Message}}}
finally{
 if($process){if(-not$process.HasExited){& "$env:SystemRoot\System32\taskkill.exe" /PID $process.Id /T /F|Out-Null;$null=$process.WaitForExit(10000)};$process.Dispose()}
 Save
}
if(-not$result.status.StartsWith('pass')){exit 1}
