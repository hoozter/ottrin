param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExpectedSid)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$out=Join-Path $Root 'results'
$result=@{status='running';runs=@();scope='Ordinary-user registry binding, anchor substitution and mixed cleanup native regressions; no durable coordinator, cross-admin bridge or enabled administrator Copy acceptance'}
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
  @{name='registry';arguments=@('windows_native_copy_registry_','--nocapture','--test-threads=1');expected='test result: ok. 6 passed; 0 failed'},
  @{name='anchor-cohort';arguments=@('windows_native_copy_cleanup_group_refuses_new_anchor_before_start','--nocapture','--test-threads=1');expected='test result: ok. 1 passed; 0 failed'},
  @{name='mixed-cohort';arguments=@('windows_native_copy_cleanup_group_mixed_','--nocapture','--test-threads=1');expected='test result: ok. 6 passed; 0 failed'}
 )
 foreach($case in $cases){
  $result.phase=$case.name;Save;$process=$null
  try {
   $info=[Diagnostics.ProcessStartInfo]::new($exe,($case.arguments-join' '));$info.UseShellExecute=$false;$info.CreateNoWindow=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true
   $info.StandardOutputEncoding=[Text.UTF8Encoding]::new($false);$info.StandardErrorEncoding=[Text.UTF8Encoding]::new($false)
   $process=[Diagnostics.Process]::Start($info);$null=$process.Handle;$stdout=$process.StandardOutput.ReadToEndAsync();$stderr=$process.StandardError.ReadToEndAsync()
   Require ($process.WaitForExit(90000)) 'Owned native test timed out';$process.WaitForExit()
   Require ($stdout.Wait(5000)-and$stderr.Wait(5000)) 'Native streams did not close'
   $run=@{case=$case.name;exit=$process.ExitCode;stdout=$stdout.Result;stderr=$stderr.Result};$result.runs+=@($run);Save
   Require ($run.exit-eq0-and$run.stdout.Contains($case.expected)) ('Native assertions failed: '+$case.name)

  }finally{if($process){if(-not$process.HasExited){& "$env:SystemRoot\System32\taskkill.exe" /PID $process.Id /T /F|Out-Null;$null=$process.WaitForExit(10000)};$process.Dispose()}}
 }
 $result.status='pass native registry binding and mixed cleanup regressions';$result.phase='complete'
}catch{$result.status='failed';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally{Save}
if(-not$result.status.StartsWith('pass')){exit 1}
