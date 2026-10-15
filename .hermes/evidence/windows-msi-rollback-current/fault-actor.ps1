$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$b='C:\OttrinDesktopQA';$r="$b\results";$controller="$b\controller.exe";$install="$env:LOCALAPPDATA\Programs\Ottrin";$app="$install\bin\ottrin.exe";$pref="$env:APPDATA\ottrin\config.json";$fixture="$env:USERPROFILE\OttrinFixtures"
$result=@{status='running';phase='prepare';scope='Exact fcf00aa MSI production uninstall failure and normal retry; ordinary account; no injected MSI action'}
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
 Assert ($env:USERNAME-eq'OttrinDesktopQA') 'Wrong user';$id=[Security.Principal.WindowsIdentity]::GetCurrent();Assert (-not(New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Elevated actor';$s=New-Object -ComObject Schedule.Service;$s.Connect();$folder=$s.GetFolder('\');$taskName='Ottrin Index '+$id.User.Value;$i=New-Object -ComObject WindowsInstaller.Installer
 $u=Get-Content "$r\upgrade-result.json" -Raw|ConvertFrom-Json;Assert ($u.status-eq'pass') 'Upgrade matrix failed';$code=$u.new_product;$result.product=$code;$result.installer_sha256=(Get-FileHash "$b\current.msi").Hash.ToLower();$prefHash=(Get-FileHash $pref).Hash;$sourceHash=(Get-FileHash "$fixture\upgrade-source.txt").Hash;$before=Live;$result.before=$before;$xml=(Task).Xml;$security=(Task).GetSecurityDescriptor(7);$result.state_before=$i.ProductState($code);Assert ($result.state_before-eq5) 'Not fully installed'
 function MSI($arguments,$name){$p=Start-Process msiexec.exe -ArgumentList "$arguments /qn /norestart /l*v $script:r\$name.log" -PassThru;$null=$p.Handle;Assert ($p.WaitForExit(180000)) "MSI remains live $($p.Id)";$exit=$p.ExitCode;$p.Dispose();return $exit}
 $cache="$env:LOCALAPPDATA\ottrin\preview_cache";New-Item -ItemType Directory $cache -Force|Out-Null;$locked=Join-Path $cache 'owned-rollback-lock';Assert (-not(Test-Path $locked)) 'Fixture lock exists';[IO.File]::WriteAllText($locked,'owned failure fixture');$handle=[IO.File]::Open($locked,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
 try{Phase 'production-failed-uninstall';$result.failure_exit=MSI "/x $code" 'failed-uninstall';Assert ($result.failure_exit-eq1603) 'Failure was not exercised'}finally{$handle.Dispose();Remove-Item -LiteralPath $locked}
 $result.state_after_failure=$i.ProductState($code);$result.task_xml_restored=((Task).Xml-eq$xml);$result.task_security_restored=((Task).GetSecurityDescriptor(7)-eq$security);$result.task_enabled=(Task).Enabled;$result.task_state=(Task).State;$result.task_instances=(Task).GetInstances(0).Count
 if($result.task_instances-eq1){$result.after=Live;$q=Run $app 'search upgrade-source --json';$result.query_total=($q.stdout|ConvertFrom-Json).total}
 $manifest=Get-Content "$b\payload.json" -Raw|ConvertFrom-Json;$result.payload_matches=$true;foreach($e in $manifest.PSObject.Properties){if(-not(Test-Path (Join-Path $install $e.Name)) -or (Get-FileHash (Join-Path $install $e.Name)).Hash.ToLower()-ne$e.Value){$result.payload_matches=$false}}
 Assert ((Get-FileHash $pref).Hash-eq$prefHash) 'Preferences changed';Assert ((Get-FileHash "$fixture\upgrade-source.txt").Hash-eq$sourceHash) 'Source changed';$result.preference_hash=$prefHash;$result.source_hash=$sourceHash
 Phase 'normal-retry-uninstall';$result.retry_uninstall_exit=MSI "/x $code" 'retry-uninstall';$result.state_after_retry=$i.ProductState($code);$result.payload_remaining_after_retry=(Test-Path $install)
 $result.status='observed';Phase 'observed-production-rollback'
}catch{$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally{$result|ConvertTo-Json -Depth 12|Set-Content -Encoding UTF8 "$r\fault-result.json"}
if($result.status-ne'observed'){exit 1}
