param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExpectedSid)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$out=Join-Path $Root 'results'
$result=@{status='running';runs=@();scope='Six individual ordinary-user mixed cleanup native regressions, each with a240-second terminal limit; no durable coordinator, cross-admin bridge or enabled administrator Copy acceptance'}
function Save { [IO.File]::WriteAllText((Join-Path $out 'RESULT.json'),($result|ConvertTo-Json -Depth 15),[Text.UTF8Encoding]::new($false)) }
function Require($value,$message){if(-not$value){throw $message}}
Save
try {
 $identity=[Security.Principal.WindowsIdentity]::GetCurrent()
 Require ($identity.User.Value-eq$ExpectedSid) 'Wrong owned user'
 Require (-not([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Ordinary token required'
 $result.identity=@{sid=$ExpectedSid;administrator=$false;session=(Get-Process -Id $PID).SessionId;local_app_data=[Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)}
 Require ($result.identity.session-gt0) 'Interactive ordinary session required'
 $result.phase='waiting for owned desktop';Save
 $deadline=[DateTime]::UtcNow.AddMinutes(8);$ready=Join-Path $out 'DESKTOP_READY.json'
 while(-not(Test-Path $ready)){Require ([DateTime]::UtcNow-lt$deadline) 'Owned desktop observation timed out';Start-Sleep -Milliseconds 250}
 Require ((Get-Content $ready -Raw|ConvertFrom-Json).sid-eq$ExpectedSid) 'Readiness belongs to another actor'
 $profileInput=Get-Content (Join-Path $Root 'application-input.json') -Raw|ConvertFrom-Json;$exe=Join-Path $Root 'boundary-test.exe'
 Require ((Get-FileHash $exe).Hash.ToLowerInvariant()-eq$profileInput.test_sha256) 'Native executable mismatch';$result.input=$profileInput
 $cases=@(
  @{name='windows_native_copy_cleanup_group_mixed_finishes_ordinary_shared_and_nested_entries';arguments=@('--exact','copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cleanup_group_mixed_finishes_ordinary_shared_and_nested_entries','--nocapture','--test-threads=1');expected='test result: ok. 1 passed; 0 failed'},
  @{name='windows_native_copy_cleanup_group_mixed_cancel_preserves_data_and_fresh_retry_completes';arguments=@('--exact','copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cleanup_group_mixed_cancel_preserves_data_and_fresh_retry_completes','--nocapture','--test-threads=1');expected='test result: ok. 1 passed; 0 failed'},
  @{name='windows_native_copy_cleanup_group_mixed_resumes_prepared_shared_owner';arguments=@('--exact','copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cleanup_group_mixed_resumes_prepared_shared_owner','--nocapture','--test-threads=1');expected='test result: ok. 1 passed; 0 failed'},
  @{name='windows_native_copy_cleanup_group_mixed_uncertain_ordinary_unlink_requires_fresh_retry';arguments=@('--exact','copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cleanup_group_mixed_uncertain_ordinary_unlink_requires_fresh_retry','--nocapture','--test-threads=1');expected='test result: ok. 1 passed; 0 failed'},
  @{name='windows_native_copy_cleanup_group_mixed_midway_cancel_resumes_acknowledged_progress';arguments=@('--exact','copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cleanup_group_mixed_midway_cancel_resumes_acknowledged_progress','--nocapture','--test-threads=1');expected='test result: ok. 1 passed; 0 failed'},
  @{name='windows_native_copy_cleanup_group_mixed_refuses_foreign_changes_during_execution';arguments=@('--exact','copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cleanup_group_mixed_refuses_foreign_changes_during_execution','--nocapture','--test-threads=1');expected='test result: ok. 1 passed; 0 failed'}
 )
 foreach($case in $cases){
  $result.phase=$case.name;Save;$process=$null
  try {
   $info=[Diagnostics.ProcessStartInfo]::new($exe,($case.arguments-join' '));$info.UseShellExecute=$false;$info.CreateNoWindow=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true
   $info.StandardOutputEncoding=[Text.UTF8Encoding]::new($false);$info.StandardErrorEncoding=[Text.UTF8Encoding]::new($false)
   $process=[Diagnostics.Process]::Start($info);$null=$process.Handle;$stdout=$process.StandardOutput.ReadToEndAsync();$stderr=$process.StandardError.ReadToEndAsync()
   $watch=[Diagnostics.Stopwatch]::StartNew();$finished=$process.WaitForExit(240000)
   if(-not$finished){& "$env:SystemRoot\System32\taskkill.exe" /PID $process.Id /T /F|Out-Null;Require ($process.WaitForExit(10000)) 'Owned timed-out native process did not stop'}
   $process.WaitForExit();$watch.Stop()
   Require ($stdout.Wait(5000)-and$stderr.Wait(5000)) 'Native streams did not close'
   $run=@{case=$case.name;exit=$process.ExitCode;elapsed_ms=$watch.ElapsedMilliseconds;timed_out=(-not$finished);stdout=$stdout.Result;stderr=$stderr.Result};$result.runs+=@($run);Save
   Require ($finished-and$run.exit-eq0-and$run.stdout.Contains($case.expected)) ('Native assertions failed: '+$case.name)

  }finally{if($process){if(-not$process.HasExited){& "$env:SystemRoot\System32\taskkill.exe" /PID $process.Id /T /F|Out-Null;$null=$process.WaitForExit(10000)};$process.Dispose()}}
 }
 $result.status='pass native registry binding and mixed cleanup regressions';$result.phase='complete'
}catch{$result.status='failed';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally{Save}
if(-not$result.status.StartsWith('pass')){exit 1}
