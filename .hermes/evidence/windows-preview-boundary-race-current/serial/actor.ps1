param([string]$Root,[string]$ExpectedSid)
$ErrorActionPreference='Stop'
$out=Join-Path $Root 'results'
$result=@{status='running';phase='ordinary token validation';runs=@();scope='Native preview Job/AppContainer boundary under a fresh ordinary interactive user; no Office conversion or package acceptance'}
function Save {$result|ConvertTo-Json -Depth 12|Set-Content -Encoding UTF8 (Join-Path $out 'RESULT.json')}
function Require($value,$message) {if(-not$value){throw $message}}
Save
try {
 $identity=[Security.Principal.WindowsIdentity]::GetCurrent()
 Require ($identity.User.Value-eq$ExpectedSid) 'Wrong owned actor SID'
 Require (-not([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Ordinary token required'
 $actorProfile=[Environment]::GetFolderPath([Environment+SpecialFolder]::UserProfile)
 Require ((Split-Path $actorProfile -Leaf)-like($identity.Name.Split('\')[-1]+'*')) 'Wrong owned profile'
 $session=(Get-Process -Id $PID).SessionId
 Require ($session-gt0) 'Interactive session required'
 $result.identity=@{sid=$ExpectedSid;administrator=$false;profile=$actorProfile;session=$session};$result.phase='waiting for observed owned desktop';Save
 $deadline=[DateTime]::UtcNow.AddMinutes(8);$ready=Join-Path $out 'DESKTOP_READY.json'
 while(-not(Test-Path $ready)){Require ([DateTime]::UtcNow-lt$deadline) 'Owned desktop was not observed';Start-Sleep -Milliseconds 250}
 Require ((Get-Content $ready -Raw|ConvertFrom-Json).sid-eq$ExpectedSid) 'Readiness belongs to another actor'
 $boundaryInput=Get-Content (Join-Path $Root 'application-input.json') -Raw|ConvertFrom-Json
 $exe=Join-Path $Root 'boundary-test.exe'
 Require ((Get-FileHash $exe).Hash.ToLowerInvariant()-eq$boundaryInput.test_sha256) 'Boundary executable input mismatch'
 $result.input=$boundaryInput;$result.desktop_ready=$true
 foreach($run in 1..3) {
  $result.phase="native boundary serial run $run";Save
  $stdout=Join-Path $out "run-$run.stdout.txt";$stderr=Join-Path $out "run-$run.stderr.txt";$process=$null
  try {
   $process=Start-Process $exe -ArgumentList @('native_job_','--nocapture','--test-threads=1') -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
   $null=$process.Handle
   if(-not$process.WaitForExit(180000)){throw "Native boundary run $run timed out"}
   $process.WaitForExit()
   $text=[IO.File]::ReadAllText($stdout);$errors=[IO.File]::ReadAllText($stderr)
   $result.runs+=@{run=$run;exit=$process.ExitCode;stdout=$text;stderr=$errors;all_ten_pass=($process.ExitCode-eq0-and$text.Contains('test result: ok. 10 passed; 0 failed'))};Save
  }finally {
   if($process){if(-not$process.HasExited){& "$env:SystemRoot\System32\taskkill.exe" /PID $process.Id /T /F|Out-Null;$null=$process.WaitForExit(10000)};$process.Dispose()}
  }
 }
 Require ($result.runs.Count-eq3) 'Three exact native observations required'
 Require (@($result.runs|Where-Object {-not$_.all_ten_pass}).Count-eq0) 'One or more native boundary runs failed; preserve all output'
 $result.status='pass';$result.phase='three serial boundary runs completed'
}catch {$result.status='failed';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally {Save}
