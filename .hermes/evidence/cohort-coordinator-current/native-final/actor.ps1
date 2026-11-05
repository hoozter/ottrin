param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExpectedSid)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$out=Join-Path $Root 'results'
$result=@{status='running';runs=@();failures=@();scope='Twenty-one ordinary-user coordinator and legacy matrix invocations covering28 named native tests, each with a600-second terminal limit; no original-caller registry bridge, alternate-admin or enabled administrator Copy acceptance'}
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
  @{name='windows_native_copy_cohort_crash_before_snapshot_publication';arguments=@('--exact','copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cohort_crash_before_snapshot_publication','--nocapture','--test-threads=1');expected='test result: ok. 1 passed; 0 failed'},
  @{name='windows_native_copy_cohort_crash_after_snapshot_publication';arguments=@('--exact','copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cohort_crash_after_snapshot_publication','--nocapture','--test-threads=1');expected='test result: ok. 1 passed; 0 failed'},
  @{name='windows_native_copy_cohort_crash_after_pending_checkpoint';arguments=@('--exact','copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cohort_crash_after_pending_checkpoint','--nocapture','--test-threads=1');expected='test result: ok. 1 passed; 0 failed'},
  @{name='windows_native_copy_cohort_crash_after_completed_checkpoint';arguments=@('--exact','copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cohort_crash_after_completed_checkpoint','--nocapture','--test-threads=1');expected='test result: ok. 1 passed; 0 failed'},
  @{name='windows_native_copy_cohort_crash_after_retirement_keeps_unknown_history_unknown';arguments=@('--exact','copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cohort_crash_after_retirement_keeps_unknown_history_unknown','--nocapture','--test-threads=1');expected='test result: ok. 1 passed; 0 failed'},
  @{name='windows_native_copy_cohort_torn_checkpoint_replays_only_validated_prefix';arguments=@('--exact','copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cohort_torn_checkpoint_replays_only_validated_prefix','--nocapture','--test-threads=1');expected='test result: ok. 1 passed; 0 failed'},
  @{name='windows_native_copy_cohort_refuses_foreign_member_log_or_parent_acl';arguments=@('--exact','copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cohort_refuses_foreign_member_log_or_parent_acl','--nocapture','--test-threads=1');expected='test result: ok. 1 passed; 0 failed'},
  @{name='windows_native_copy_cohort_preserves_lossless_native_paths';arguments=@('--exact','copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cohort_preserves_lossless_native_paths','--nocapture','--test-threads=1');expected='test result: ok. 1 passed; 0 failed'},
  @{name='windows_native_copy_cohort_crash_after_all_completed_checkpoints';arguments=@('--exact','copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cohort_crash_after_all_completed_checkpoints','--nocapture','--test-threads=1');expected='test result: ok. 1 passed; 0 failed'},
  @{name='windows_native_copy_cohort_refuses_tail_after_completed_prefix';arguments=@('--exact','copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cohort_refuses_tail_after_completed_prefix','--nocapture','--test-threads=1');expected='test result: ok. 1 passed; 0 failed'},
  @{name='windows_native_copy_cleanup_group_metadata_finishes_members_with_bound_catalog_handoffs';arguments=@('--exact','copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cleanup_group_metadata_finishes_members_with_bound_catalog_handoffs','--nocapture','--test-threads=1');expected='test result: ok. 1 passed; 0 failed'},
  @{name='windows_native_copy_cleanup_group_metadata_preserves_foreign_changes_while_pending';arguments=@('--exact','copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cleanup_group_metadata_preserves_foreign_changes_while_pending','--nocapture','--test-threads=1');expected='test result: ok. 1 passed; 0 failed'},
  @{name='windows_native_copy_cleanup_configured_group_action_reclaims_mixed_cohort';arguments=@('--exact','copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cleanup_configured_group_action_reclaims_mixed_cohort','--nocapture','--test-threads=1');expected='test result: ok. 1 passed; 0 failed'},
  @{name='windows_native_copy_cleanup_configured_registry_writer_supports_registry_destinations';arguments=@('--exact','copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cleanup_configured_registry_writer_supports_registry_destinations','--nocapture','--test-threads=1');expected='test result: ok. 1 passed; 0 failed'},
  @{name='windows_native_copy_cleanup_group_metadata_retains_authority_through_uncertain_removal';arguments=@('--exact','copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cleanup_group_metadata_retains_authority_through_uncertain_removal','--nocapture','--test-threads=1');expected='test result: ok. 1 passed; 0 failed'},
  @{name='windows_native_copy_cleanup_group_process_exit_metadata_and_history_recovery';arguments=@('--exact','copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cleanup_group_process_exit_metadata_and_history_recovery','--nocapture','--test-threads=1');expected='test result: ok. 1 passed; 0 failed'},
  @{name='windows_native_copy_cleanup_group_metadata_executor_prepares_peers_before_first_unlink';arguments=@('--exact','copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cleanup_group_metadata_executor_prepares_peers_before_first_unlink','--nocapture','--test-threads=1');expected='test result: ok. 1 passed; 0 failed'},
  @{name='windows_native_copy_cleanup_group_metadata_preparation_keeps_all_controls_reopenable';arguments=@('--exact','copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cleanup_group_metadata_preparation_keeps_all_controls_reopenable','--nocapture','--test-threads=1');expected='test result: ok. 1 passed; 0 failed'},
  @{name='registry';arguments=@('copy_transaction::windows::registry::tests::windows_native_copy_registry_','--nocapture','--test-threads=1');expected='test result: ok. 6 passed; 0 failed'},
  @{name='claims';arguments=@('copy_transaction::windows::registry::tests::windows_native_copy_claim_namespace_','--nocapture','--test-threads=1');expected='test result: ok. 3 passed; 0 failed'}
 )
 foreach($case in $cases){
  $result.phase=$case.name;Save;$process=$null
  try {
   $info=[Diagnostics.ProcessStartInfo]::new($exe,($case.arguments-join' '));$info.UseShellExecute=$false;$info.CreateNoWindow=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true
   $info.StandardOutputEncoding=[Text.UTF8Encoding]::new($false);$info.StandardErrorEncoding=[Text.UTF8Encoding]::new($false)
   $process=[Diagnostics.Process]::Start($info);$null=$process.Handle;$stdout=$process.StandardOutput.ReadToEndAsync();$stderr=$process.StandardError.ReadToEndAsync()
   $watch=[Diagnostics.Stopwatch]::StartNew();$finished=$process.WaitForExit(600000)
   if(-not$finished){& "$env:SystemRoot\System32\taskkill.exe" /PID $process.Id /T /F|Out-Null;Require ($process.WaitForExit(10000)) 'Owned timed-out native process did not stop'}
   $process.WaitForExit();$watch.Stop()
   Require ($stdout.Wait(5000)-and$stderr.Wait(5000)) 'Native streams did not close'
   $run=@{case=$case.name;exit=$process.ExitCode;elapsed_ms=$watch.ElapsedMilliseconds;timed_out=(-not$finished);stdout=$stdout.Result;stderr=$stderr.Result};$result.runs+=@($run);Save
   if(-not($finished-and$run.exit-eq0-and$run.stdout.Contains($case.expected))){$result.failures+=@($case.name);Save}

  }finally{if($process){if(-not$process.HasExited){& "$env:SystemRoot\System32\taskkill.exe" /PID $process.Id /T /F|Out-Null;$null=$process.WaitForExit(10000)};$process.Dispose()}}
 }
 Require ($result.failures.Count-eq0) ('Native assertions failed: '+($result.failures-join', '))
 $result.status='pass native external cohort and cleanup regressions';$result.phase='complete'
}catch{$result.status='failed';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally{Save}
if(-not$result.status.StartsWith('pass')){exit 1}
