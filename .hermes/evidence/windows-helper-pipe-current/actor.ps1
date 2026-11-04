param([ValidateSet('basic','cancel','approve')][string]$Mode='basic')
$ErrorActionPreference='Stop';$root='C:\OttrinHelperPipeQA';$process=$null
$result=@{status='running';mode=$Mode}
try {
 if($env:USERNAME-ne'campbell'){throw 'Wrong owned native desktop account'}
 $identity=[Security.Principal.WindowsIdentity]::GetCurrent();$admin=([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
 if($admin){throw 'Fixture must run with a filtered ordinary desktop token'}
 $result.ordinary_token=$true;$result.sid=$identity.User.Value
 $env:TEMP="$root\ordinary temp Å";$env:TMP=$env:TEMP
 $env:OTTRIN_QA_PRIV_HELPER="$root\package space Å\ottrin-priv-helper.exe"
 $env:OTTRIN_QA_ADMIN_DIRECTORY="$root\admin-only";$env:OTTRIN_QA_UAC_EXPECT=$Mode
 $runs=@()
 if($Mode-eq'basic'){
  & $env:OTTRIN_QA_PRIV_HELPER --check-protocol 2 --local-pipe;if($LASTEXITCODE-ne0){throw 'Pipe feature probe failed'}
  & $env:OTTRIN_QA_PRIV_HELPER --check-protocol 2 --copy-recovery;if($LASTEXITCODE-ne69){throw 'Windows Copy must remain unsupported'}
  & $env:OTTRIN_QA_PRIV_HELPER --request-file "$root\never-request.json" --response-file "$root\never-response.json";if($LASTEXITCODE-ne2){throw 'Superseded file transport was accepted'}
  if((Test-Path "$root\never-request.json")-or(Test-Path "$root\never-response.json")){throw 'Rejected mode created files'}
  $commands=@(@('ottrin_platform','windows_helper_pipe --nocapture --test-threads=1'),@('ottrin_search','windows_pipe --nocapture --test-threads=1'),@('ottrin_platform','windows_privileged_transport::tests::windows_helper_pipe_native_helper_round_trip_and_parent_rejection --ignored --exact --nocapture --test-threads=1'))
 }else{$commands=,@('ottrin_platform','windows_privileged_transport::tests::windows_helper_pipe_native_uac_round_trip --ignored --exact --nocapture --test-threads=1')}
 $index=0
 foreach($command in $commands){
  $index++;$stdout="$root\results\$Mode-$index.stdout";$stderr="$root\results\$Mode-$index.stderr"
  $process=Start-Process -FilePath "$root\$($command[0]).exe" -ArgumentList $command[1] -PassThru -WorkingDirectory $root -RedirectStandardOutput $stdout -RedirectStandardError $stderr
  $null=$process.Handle
  if(-not$process.WaitForExit(180000)){throw "Owned native fixture still live: $($process.Id)"}
  $runs+=@{binary=$command[0];arguments=$command[1];exit=$process.ExitCode}
  if($process.ExitCode-ne0){throw "Native fixture failed: $($command[1])"}
  $process=$null
 }
 $result.runs=$runs
 $result.temporary_entries=@(Get-ChildItem -LiteralPath $env:TEMP -Force).Count
 if($result.temporary_entries){throw 'Native fixtures left temporary entries'}
 $result.status='pass'
}catch{$result.status='fail';$result.error=$_.Exception.Message}
finally{
 if($process-and-not$process.HasExited){& C:\Windows\System32\taskkill.exe /PID $process.Id /T /F|Out-Null;$null=$process.WaitForExit(10000)}
 $result|ConvertTo-Json -Depth 8|Set-Content "$root\results\$Mode.json"
}
if($result.status-ne'pass'){exit 1}
