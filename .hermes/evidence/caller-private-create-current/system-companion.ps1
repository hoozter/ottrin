param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExpectedSid)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$process=$null
$out=Join-Path $Root 'results';$result=@{status='starting';runs=@();scope='Owned SYSTEM caller-creation role and native Copy security regressions including privilege-dependent link/audit cases; no UAC or enabled Copy acceptance'}
function Require($value,$message){if(-not$value){throw $message}}
function Publish([string]$Name,$value){$path=Join-Path $out $Name;$temp=$path+'.preparing';[IO.File]::WriteAllText($temp,($value|ConvertTo-Json -Depth 12),[Text.UTF8Encoding]::new($false));Move-Item -LiteralPath $temp -Destination $path -ErrorAction Stop}
try {
 Require ([Security.Principal.WindowsIdentity]::GetCurrent().User.Value-eq'S-1-5-18') 'SYSTEM companion required'
 $probeInput=Get-Content (Join-Path $Root 'application-input.json') -Raw|ConvertFrom-Json
 Require ((Get-FileHash (Join-Path $Root 'system-companion.ps1')).Hash.ToLowerInvariant()-eq$probeInput.system_companion_sha256) 'Companion source mismatch'
 $exe=Join-Path $Root 'boundary-test.exe';Require ((Get-FileHash $exe).Hash.ToLowerInvariant()-eq$probeInput.test_sha256) 'Native executable mismatch'
 $ready=Join-Path $out 'CALLER_READY.json';$deadline=[DateTime]::UtcNow.AddMinutes(15)
 while(-not(Test-Path -LiteralPath $ready)){Require ([DateTime]::UtcNow-lt$deadline) 'Owned ordinary client did not start';Start-Sleep -Milliseconds 200}
 $caller=Get-Content $ready -Raw|ConvertFrom-Json
 $original=Get-CimInstance Win32_Process -Filter ('ProcessId='+[uint32]$caller.pid)
 Require ($null-ne$original) 'Original owned process missing'
 Require ((Invoke-CimMethod -InputObject $original -MethodName GetOwnerSid).Sid-eq$ExpectedSid) 'Original process is not the fresh owned user'
 foreach($case in @(@{name='caller private helper';arguments='--exact windows_privileged_transport::tests::caller_owner::system_helper --ignored --nocapture --test-threads=1';expected='test result: ok. 1 passed; 0 failed';publish=$true},@{name='SYSTEM native symbolic-link security';arguments='--exact copy_transaction::windows::security::tests::windows_native_copy_final_link_dacl_does_not_change_populated_targets --nocapture --test-threads=1';expected='test result: ok. 1 passed; 0 failed';publish=$false},@{name='SYSTEM native audit-SACL security';arguments='--exact copy_transaction::windows::security::tests::new_copy_candidate_inherits_the_real_parent_audit_sacl --ignored --nocapture --test-threads=1';expected='test result: ok. 1 passed; 0 failed';publish=$false})){
 $info=[Diagnostics.ProcessStartInfo]::new($exe,$case.arguments)
 $info.UseShellExecute=$false;$info.CreateNoWindow=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true
 $info.StandardOutputEncoding=[Text.UTF8Encoding]::new($false);$info.StandardErrorEncoding=[Text.UTF8Encoding]::new($false)
 $info.EnvironmentVariables['OTTRIN_CALLER_PROBE_ROOT']=$Root
 $info.EnvironmentVariables['LOCALAPPDATA']=Join-Path $Root 'poisoned-system-profile'
 $info.EnvironmentVariables['USERPROFILE']=Join-Path $Root 'poisoned-system-profile'
 $process=[Diagnostics.Process]::Start($info);$null=$process.Handle
 if($case.publish){Publish 'SYSTEM_PROCESS.json' @{pid=$process.Id;expected_caller_sid=$ExpectedSid;source_sha256=$probeInput.test_sha256}}
 $stdout=$process.StandardOutput.ReadToEndAsync();$stderr=$process.StandardError.ReadToEndAsync()
 Require ($process.WaitForExit(180000)) 'Owned SYSTEM native test timed out';$process.WaitForExit()
 Require ($stdout.Wait(5000)-and$stderr.Wait(5000)) 'SYSTEM test streams did not close'
 $result.runs+=@{case=$case.name;exit=$process.ExitCode;stdout=$stdout.Result;stderr=$stderr.Result}
 Require ($process.ExitCode-eq0-and$stdout.Result.Contains($case.expected)) ('SYSTEM native assertions failed: '+$case.name)
 $process.Dispose();$process=$null
 }
 Require (-not(Test-Path (Join-Path $Root 'poisoned-system-profile'))) 'Poisoned profile was created'
 $result.status='pass owned SYSTEM companion'
}catch{$result.status='failed';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally{
 if($process){if(-not$process.HasExited){& "$env:SystemRoot\System32\taskkill.exe" /PID $process.Id /T /F|Out-Null;$null=$process.WaitForExit(10000)};$process.Dispose()}
 Publish 'SYSTEM_RESULT.json' $result
}
if(-not$result.status.StartsWith('pass')){exit 1}
