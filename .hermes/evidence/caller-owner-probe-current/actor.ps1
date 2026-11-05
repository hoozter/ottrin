param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExpectedSid)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$process=$null;$out=Join-Path $Root 'results'
$result=@{status='running';runs=@();scope='Ordinary-client/SYSTEM-companion original-caller and owner-creation mechanism observation; no alternate-administrator UAC, production Copy, registry bridge or package acceptance'}
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
 $result.phase='native cross-principal client';Save
 $info=[Diagnostics.ProcessStartInfo]::new($exe,'--exact windows_privileged_transport::tests::caller_owner::client --ignored --nocapture --test-threads=1')
 $info.UseShellExecute=$false;$info.CreateNoWindow=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true
 $info.StandardOutputEncoding=[Text.UTF8Encoding]::new($false);$info.StandardErrorEncoding=[Text.UTF8Encoding]::new($false)
 $info.EnvironmentVariables['OTTRIN_CALLER_PROBE_ROOT']=$Root
 $process=[Diagnostics.Process]::Start($info);$null=$process.Handle;$stdout=$process.StandardOutput.ReadToEndAsync();$stderr=$process.StandardError.ReadToEndAsync()
 Require ($process.WaitForExit(180000)) 'Owned ordinary native test timed out';$process.WaitForExit()
 Require ($stdout.Wait(5000)-and$stderr.Wait(5000)) 'Ordinary test streams did not close'
 $result.runs+=@{case='ordinary client';exit=$process.ExitCode;stdout=$stdout.Result;stderr=$stderr.Result};Save
 Require ($process.ExitCode-eq0-and$stdout.Result.Contains('test result: ok. 1 passed; 0 failed')) 'Ordinary native assertions failed'
 $result.phase='waiting for owned SYSTEM completion';Save;$done=Join-Path $out 'SYSTEM_RESULT.json';$deadline=[DateTime]::UtcNow.AddMinutes(2)
 while(-not(Test-Path $done)){Require ([DateTime]::UtcNow-lt$deadline) 'SYSTEM result missing';Start-Sleep -Milliseconds 200}
 $system=Get-Content $done -Raw|ConvertFrom-Json;$result.system=$system;Require ($system.status.StartsWith('pass')) 'SYSTEM companion failed'
 $result.observation=Get-Content (Join-Path $out 'OWNER_OBSERVATION.json') -Raw|ConvertFrom-Json
 $result.status='pass cross-principal native mechanism observation';$result.phase='complete'
}catch{$result.status='failed';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace;$systemResult=Join-Path $out 'SYSTEM_RESULT.json';if(Test-Path $systemResult){try{$result.system=Get-Content $systemResult -Raw|ConvertFrom-Json}catch{$result.system_diagnostic_error=$_.Exception.Message}}}
finally{
 if($process){if(-not$process.HasExited){& "$env:SystemRoot\System32\taskkill.exe" /PID $process.Id /T /F|Out-Null;$null=$process.WaitForExit(10000)};$process.Dispose()}
 Save
}
if(-not$result.status.StartsWith('pass')){exit 1}
