$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$b='C:\OttrinDesktopQA';$r="$b\results";$controller="$b\controller.exe";$install="$env:LOCALAPPDATA\Programs\Ottrin";$app="$install\bin\ottrin.exe";$pref="$env:APPDATA\ottrin\config.json";$fixture="$env:USERPROFILE\OttrinFixtures"
$result=@{status='running';phase='prepare';scope='Source-built transaction controller; published410 MSI baseline; ordinary interactive user'}
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
 $prefHash=(Get-FileHash $pref).Hash;$sourceHash=(Get-FileHash "$fixture\upgrade-source.txt").Hash
 $s=New-Object -ComObject Schedule.Service;$s.Connect();$folder=$s.GetFolder('\');$taskName='Ottrin Index '+$id.User.Value
 $t=Task;$t.Enabled=$true;$null=Run $app 'index service-install';$before=Live;$xml=(Task).Xml;$security=(Task).GetSecurityDescriptor(7);$result.before=$before
 $files=@{};Get-ChildItem $install -Recurse -File|ForEach-Object {$files[$_.FullName]=(Get-FileHash $_.FullName).Hash}
 function Same-Payload {foreach($path in $script:files.Keys){Assert ((Get-FileHash $path).Hash-eq$script:files[$path]) "Failure changed $path"}}
 Phase 'injected-abort-after-quiesce';$out=Run "$b\failure-setup.exe" '/S' $false;Assert ($out.exit-eq73) "Injected abort returned $($out.exit)";$result.failure_exit=$out.exit;$result.after=Live;Assert ($before.pid-ne$result.after.pid) 'Quiescence was not exercised';Assert ((Task).Xml-eq$xml-and(Task).GetSecurityDescriptor(7)-eq$security) 'Callback did not restore exact task';Assert (-not(Test-Path "$env:LOCALAPPDATA\ottrin\installer-index-transaction.json")) 'Restored journal remains';Same-Payload;Check-Data
 Phase 'invalid-journal-refusal';$journal="$env:LOCALAPPDATA\ottrin\installer-index-transaction.json";[IO.File]::WriteAllText($journal,'owned invalid fixture');$prior=Live;$out=Run "$b\fixed-setup.exe" '/S' $false;Assert ($out.exit-eq1) "Refusal returned $($out.exit)";Assert ([IO.File]::ReadAllText($journal)-eq'owned invalid fixture') 'Invalid journal lost';$now=Live;Assert ($now.pid-eq$prior.pid) 'Refusal stopped worker';Same-Payload;Remove-Item -LiteralPath $journal;Check-Data;$result.invalid_refusal_exit=$out.exit
 function Remove-Install {
  $null=Run "$script:install\Uninstall.exe" '/S';$deadline=[DateTime]::UtcNow.AddSeconds(45);while((Test-Path $script:install)-and[DateTime]::UtcNow-lt$deadline){Start-Sleep -Milliseconds 200};Assert (-not(Test-Path $script:install)) 'Uninstall incomplete';Assert (-not(Test-Path "$env:LOCALAPPDATA\ottrin\search.db")) 'Index remains';Assert ((Get-FileHash $script:pref).Hash-eq$script:prefHash) 'Preference changed';Assert ((Get-FileHash "$script:fixture\upgrade-source.txt").Hash-eq$script:sourceHash) 'Source changed'
 }
 Phase 'uninstall-before-fresh';Remove-Install
 Phase 'fresh-fixed-install';$null=Run "$b\fixed-setup.exe" '/S';$result.fresh=Live;Assert ((Task).Enabled) 'Fresh login service disabled';$null=Run $app 'index update --json';Check-Data
 Phase 'fresh-uninstall';Remove-Install;$result.status='pass';$result.failure_setup_sha256=(Get-FileHash "$b\failure-setup.exe").Hash.ToLower();$result.fixed_setup_sha256=(Get-FileHash "$b\fixed-setup.exe").Hash.ToLower();Phase 'uninstalled'
}catch{$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally{$result|ConvertTo-Json -Depth 12|Set-Content "$r\nsis-failure-result.json"}
if($result.status-ne'pass'){exit 1}
