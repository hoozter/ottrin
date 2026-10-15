$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$b='C:\OttrinDesktopQA';$r="$b\results";$controller="$b\controller.exe";$install="$env:LOCALAPPDATA\Programs\Ottrin";$app="$install\bin\ottrin.exe";$pref="$env:APPDATA\ottrin\config.json";$fixture="$env:USERPROFILE\OttrinFixtures"
$result=@{status='running';phase='prepare';scope='Clean fcf00aa NSIS published410 running upgrade and intent matrix; ordinary interactive user'}
function Assert($ok,$msg){if(-not$ok){throw $msg}}
function Run($program,$arguments,$expectSuccess=$true){
 $s=New-Object Diagnostics.ProcessStartInfo($program,$arguments);$s.UseShellExecute=$false;$s.RedirectStandardOutput=$true;$s.RedirectStandardError=$true;$s.CreateNoWindow=$true;$p=[Diagnostics.Process]::Start($s)
 try{$o=$p.StandardOutput.ReadToEndAsync();$e=$p.StandardError.ReadToEndAsync();Assert ($p.WaitForExit(60000)) "Owned process timeout $($p.Id)";Assert ($o.Wait(5000)-and$e.Wait(5000)) 'Output remains open';$code=$p.ExitCode;Assert (($code-eq0)-eq$expectSuccess) "Unexpected exit $code : $arguments : $($e.Result)";return @{exit=$code;stdout=$o.Result;stderr=$e.Result}}
 finally{if(-not$p.HasExited){$p.Kill();$null=$p.WaitForExit(5000)};$p.Dispose()}
}
function Phase($value){$result.phase=$value;$result|ConvertTo-Json -Depth 8|Set-Content "$r\controller-status.json"}
function Task { $script:folder.GetTask($script:taskName) }
function Live {
 $t=Task;Assert ($t.GetInstances(0).Count-eq1 -and $t.State-eq4) 'Expected one running index task'
 $p=@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue|Where-Object {$_.Path-eq"$script:install\bin\ottrin-indexd.exe"});Assert ($p.Count-eq1) 'Expected one installed daemon';return @{pid=$p[0].Id;instance=$t.GetInstances(0).Item(1).InstanceGuid}
}
function Check-Data {
 Assert ((Get-FileHash $script:pref).Hash-eq$script:prefHash) 'Preferences changed'
 Assert ((Get-FileHash "$script:fixture\upgrade-source.txt").Hash-eq$script:sourceHash) 'Source changed'
 $query=(Run $script:app 'search upgrade-source --json').stdout|ConvertFrom-Json;Assert ($query.total-eq1) 'Saved index query missing'
}
try {
 Assert ($env:USERNAME-eq'OttrinDesktopQA') 'Wrong account';$id=[Security.Principal.WindowsIdentity]::GetCurrent();Assert (-not(New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Elevated actor'
 # Remove the rejected mixed-payload fixture through its real uninstaller.
 if(Test-Path "$install\Uninstall.exe"){
  $null=Run "$install\Uninstall.exe" '/S';$deadline=[DateTime]::UtcNow.AddSeconds(40);while((Test-Path $install)-and[DateTime]::UtcNow-lt$deadline){Start-Sleep -Milliseconds 200};Assert (-not(Test-Path $install)) 'Previous fixture uninstall incomplete'
 }
 $prefHash=(Get-FileHash $pref).Hash;$sourceHash=(Get-FileHash "$fixture\upgrade-source.txt").Hash
 $s=New-Object -ComObject Schedule.Service;$s.Connect();$folder=$s.GetFolder('\');$taskName='Ottrin Index '+$id.User.Value
 Phase 'nsis-baseline-install';$null=Run "$b\baseline-setup.exe" '/S';$null=Run $app 'index update --json';Check-Data;$result.before=Live
 $oldHash=(Get-FileHash "$install\bin\ottrin-indexd.exe").Hash.ToLower();$manifest=Get-Content "$b\new-payload.json" -Raw|ConvertFrom-Json;$expected=$manifest.'bin/ottrin-indexd.exe';if(-not$expected){$expected=$manifest.'bin\ottrin-indexd.exe'};Assert ($oldHash-ne$expected) 'No changed daemon to exercise'
 $result.before_enabled=(Task).Enabled;$result.before_security=(Task).GetSecurityDescriptor(7);$dbpath="$env:LOCALAPPDATA\ottrin\search.db";$dbCreated=[IO.File]::GetCreationTimeUtc($dbpath).Ticks
 function Verify-Payload {
  foreach($e in $script:manifest.PSObject.Properties){Assert ((Get-FileHash (Join-Path $script:install $e.Name)).Hash.ToLower()-eq$e.Value) "Payload differs: $($e.Name)"}
  Assert (@(Get-ChildItem $script:install -Recurse -File).Count-eq(@($script:manifest.PSObject.Properties).Count+1)) 'Unexpected payload count including NSIS uninstaller'
  Assert (-not(Test-Path "$env:LOCALAPPDATA\ottrin\installer-index-transaction.json")) 'Committed journal remains';Check-Data
 }
 Phase 'nsis-running-upgrade';$upgrade=Run "$b\fixed-setup.exe" '/S';$result.setup_exit=$upgrade.exit;$result.after=Live;Verify-Payload
 Assert ($result.before.pid-ne$result.after.pid) 'Daemon not restarted';Assert ((Task).Enabled-eq$result.before_enabled) 'Login state changed';Assert ((Task).GetSecurityDescriptor(7)-eq$result.before_security) 'Security changed';Assert ([IO.File]::GetCreationTimeUtc($dbpath).Ticks-eq$dbCreated) 'Database replaced'
 $result.runtime_cases=@()
 foreach($case in @(@{enabled=$false;running=$true},@{enabled=$true;running=$false},@{enabled=$false;running=$false})){
  Phase "nsis-reinstall-$($case.enabled)-$($case.running)";$t=Task;$t.Enabled=$case.enabled;if(-not$case.running){$null=Run $controller '--stop'}
  $null=Run "$b\fixed-setup.exe" '/S';Assert ((Task).Enabled-eq$case.enabled) 'Reinstall changed login intent';Assert (((Task).GetInstances(0).Count-gt0)-eq$case.running) 'Reinstall changed runtime intent';Verify-Payload;$result.runtime_cases+=@{enabled=$case.enabled;running=$case.running;status='pass'}
 }
 $result.installer_sha256=(Get-FileHash "$b\fixed-setup.exe").Hash.ToLower();$result.payload_files=@($manifest.PSObject.Properties).Count;$result.preference_hash=$prefHash;$result.source_hash=$sourceHash;$result.status='pass';Phase 'accepted'
}catch{$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally{$result|ConvertTo-Json -Depth 12|Set-Content "$r\nsis-fixed-result.json"}
if($result.status-ne'pass'){exit 1}
