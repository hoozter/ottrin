param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExpectedSid)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$out=Join-Path $Root 'results'
$result=@{status='running';runs=@();failures=@();scope='Native ordinary-user Activity/controller recovery with real egui input and platform fixtures; no packaged/native-window visual, original-caller privileged bridge or enabled administrator Copy acceptance'}
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
 $fixture=Join-Path $Root 'platform-fixture-test.exe'
 Require ((Get-FileHash $fixture).Hash.ToLowerInvariant()-eq$profileInput.platform_fixture.sha256) 'Platform fixture executable mismatch'
 $env:OTTRIN_TEST_WINDOWS_PLATFORM_FIXTURE_EXE=$fixture
 $env:OTTRIN_TEST_WINDOWS_COPY_ACTIVITY_ROOT=$null
 $env:OTTRIN_TEST_WINDOWS_COPY_ACTIVITY_CASE=$null
 $cases=@(
  @{name='mutation-fence';executable=$fixture;arguments=@('--exact','copy_transaction::windows::journal::tests::cleanup::windows_native_copy_cohort_fences_new_copy_and_unselected_undo','--nocapture','--test-threads=1');expected='test result: ok. 1 passed; 0 failed'},
  @{name='activity';executable=$exe;arguments=@('--exact','tests::windows_copy_recovery_activity_integration','--ignored','--nocapture','--test-threads=1');expected='test result: ok. 1 passed; 0 failed'},
  @{name='readonly';executable=$exe;arguments=@('--exact','tests::windows_retained_copy_activity_missing_registry_worker_is_read_only','--nocapture','--test-threads=1');expected='test result: ok. 1 passed; 0 failed'}
 )
 foreach($case in $cases){
  $result.phase=$case.name;Save;$process=$null
  try {
   $info=[Diagnostics.ProcessStartInfo]::new($case.executable,($case.arguments-join' '));$info.UseShellExecute=$false;$info.CreateNoWindow=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true
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
 $result.status='pass native mutation fence and Activity recovery';$result.phase='complete'
}catch{$result.status='failed';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally{Save}
if(-not$result.status.StartsWith('pass')){exit 1}
