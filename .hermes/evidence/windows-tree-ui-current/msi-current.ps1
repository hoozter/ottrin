$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$b='C:\OttrinTreeUIQA';$r="$b\results";$install="$env:LOCALAPPDATA\Programs\Ottrin";$app="$install\bin\ottrin.exe";$pref="$env:APPDATA\ottrin\config.json";$processes=@();$code=$null;$result=@{status='running';scope='Current MSI ordinary-user install, payload, explicit scheduled daemon setup, watching, populated-tree rename, quiet-fixture CPU and normal uninstall; no failed-registration rollback claim'}
function Assert($ok,$message){if(-not$ok){throw $message}}
function Run($program,$arguments){
 $s=New-Object Diagnostics.ProcessStartInfo($program,$arguments);$s.UseShellExecute=$false;$s.RedirectStandardOutput=$true;$s.RedirectStandardError=$true;$s.CreateNoWindow=$true;$p=[Diagnostics.Process]::Start($s);$script:processes+=,$p
 $o=$p.StandardOutput.ReadToEndAsync();$e=$p.StandardError.ReadToEndAsync();Assert ($p.WaitForExit(120000)) "Owned command remains live $($p.Id)";Assert ($o.Wait(5000)-and$e.Wait(5000)) 'Output remains open';Assert ($p.ExitCode-eq0) "Command failed $arguments : $($e.Result)";return $o.Result
}
function Msi($action,$label){$p=Start-Process msiexec.exe -ArgumentList "$action /qn /norestart /l*v $r\$label.log" -PassThru;$null=$p.Handle;$script:processes+=,$p;Assert ($p.WaitForExit(180000)) "Owned MSI client remains live $($p.Id)";Assert ($p.ExitCode-eq0) "MSI failed $label : $($p.ExitCode)";return $p.ExitCode}
function WaitPath($query,$expected){$until=[DateTime]::UtcNow.AddSeconds(15);do{$answer=(Run $app "search $query --json")|ConvertFrom-Json;$actual=if($answer.total-eq1){$answer.items[0].path}else{''};if($actual.StartsWith('\\?\')){$actual=$actual.Substring(4)};if($answer.total-eq1-and$actual-eq$expected){return};Start-Sleep -Milliseconds 150}while([DateTime]::UtcNow-lt$until);throw "Watcher did not follow $query"}
try {
 Assert ($env:USERNAME-eq'OttrinTreeUIQA') 'Wrong account';Assert (-not(Test-Path $install)) 'Unexpected installation'
 Assert ((Get-Content "$r\FINAL_STATE.json" -Raw -Encoding UTF8|ConvertFrom-Json).status-eq'pass') 'Matrix not independently verified'
 $fixture="$env:USERPROFILE\MsiCurrentWatch";Assert (-not(Test-Path $fixture)) 'Fixture exists';New-Item -ItemType Directory "$fixture\before\nested"|Out-Null;[IO.File]::WriteAllText("$fixture\before\nested\msi-probe.txt",'MSI watcher bytes')
 $cfg=Get-Content $pref -Raw -Encoding UTF8|ConvertFrom-Json;$cfg.search.include_roots=@($fixture);[IO.File]::WriteAllText($pref,($cfg|ConvertTo-Json -Depth 40),[Text.UTF8Encoding]::new($false));$prefHash=(Get-FileHash $pref).Hash
 $i=New-Object -ComObject WindowsInstaller.Installer;$db=$i.OpenDatabase("$b\current.msi",0);$v=$db.OpenView("SELECT ``Value`` FROM ``Property`` WHERE ``Property``='ProductCode'");$null=$v.Execute();$code=$v.Fetch().StringData(1);$v.Close()
 $result.install_exit=Msi "/i $b\current.msi" 'msi-current-install';Assert ($i.ProductState($code)-eq5) 'MSI registration absent'
 $manifest=Get-Content "$b\payload.json" -Raw|ConvertFrom-Json;foreach($entry in $manifest.PSObject.Properties){Assert ((Get-FileHash (Join-Path $install $entry.Name)).Hash.ToLowerInvariant()-ceq$entry.Value) "Payload differs: $($entry.Name)"};$result.payload_files=@($manifest.PSObject.Properties).Count;Assert (@(Get-ChildItem $install -File -Recurse).Count-eq$result.payload_files) 'Unexpected MSI payload files'
 $null=Run $app 'index service-install';$status=(Run $app 'index update --json')|ConvertFrom-Json;Assert ($status.status-eq'ready'-and$status.configured_roots-eq1) 'Private index not ready'
 WaitPath 'msi-probe' "$fixture\before\nested\msi-probe.txt";[IO.Directory]::Move("$fixture\before","$fixture\after");WaitPath 'msi-probe' "$fixture\after\nested\msi-probe.txt";Assert ([IO.File]::ReadAllText("$fixture\after\nested\msi-probe.txt")-ceq'MSI watcher bytes') 'Source changed'
 $s=New-Object -ComObject Schedule.Service;$s.Connect();$name='Ottrin Index '+[Security.Principal.WindowsIdentity]::GetCurrent().User.Value;$t=$s.GetFolder('\').GetTask($name);Assert ($t.State-eq4-and$t.GetInstances(0).Count-eq1) 'Expected one running scheduled daemon'
 $workers=@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue|Where-Object {$_.Path-eq"$install\bin\ottrin-indexd.exe"});Assert ($workers.Count-eq1) 'Expected one own index daemon'
 Start-Sleep -Seconds 2;$worker=$workers[0];$worker.Refresh();$cpu=$worker.TotalProcessorTime.TotalMilliseconds;$timer=[Diagnostics.Stopwatch]::StartNew();Start-Sleep -Seconds 5;$worker.Refresh();$result.quiet_cpu_single_core_percent=[Math]::Round(100*($worker.TotalProcessorTime.TotalMilliseconds-$cpu)/$timer.Elapsed.TotalMilliseconds,4);Assert ($result.quiet_cpu_single_core_percent-lt2) 'Quiet private fixture CPU exceeds bound'
 $result.uninstall_exit=Msi "/x $code" 'msi-current-uninstall';Assert ($i.ProductState($code)-eq-1-and-not(Test-Path $install)) 'MSI uninstall incomplete';Assert (-not(Test-Path "$env:LOCALAPPDATA\ottrin\search.db")) 'Index remains';Assert ((Get-FileHash $pref).Hash-eq$prefHash) 'Preferences changed';Assert ([IO.File]::ReadAllText("$fixture\after\nested\msi-probe.txt")-ceq'MSI watcher bytes') 'Source changed during uninstall'
 $absent=$false;try{$null=$s.GetFolder('\').GetTask($name)}catch{if($_.Exception.GetBaseException().HResult-eq-2147024894){$absent=$true}else{throw}};Assert $absent 'Own index task remains'
 $result.installer_sha256=(Get-FileHash "$b\current.msi").Hash.ToLowerInvariant();$result.status='pass'
} catch {$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally {
 $errors=@();if(Test-Path "$install\bin\ottrin-indexd.exe"){try{$null=Run "$install\bin\ottrin-indexd.exe" '--stop';if($code-and$i.ProductState($code)-eq5){$null=Msi "/x $code" 'msi-current-recovery-uninstall'}}catch{$errors+=$_.Exception.Message}}
 foreach($p in $processes){if(-not$p.HasExited){& "$env:WINDIR\System32\taskkill.exe" /PID $p.Id /T /F|Out-Null;if(-not$p.WaitForExit(10000)){$errors+='Owned process remains'}}}
 if($errors.Count){$result.status='fail';$result.cleanup_errors=$errors};$result|ConvertTo-Json -Depth 12|Set-Content "$r\MSI_CURRENT.json" -Encoding UTF8
}
if($result.status-ne'pass'){exit 1}
