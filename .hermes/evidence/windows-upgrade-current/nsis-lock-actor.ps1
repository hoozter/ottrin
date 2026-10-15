$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$b='C:\OttrinDesktopQA';$r="$b\results";$controller="$b\controller.exe";$install="$env:LOCALAPPDATA\Programs\Ottrin";$app="$install\bin\ottrin.exe";$pref="$env:APPDATA\ottrin\config.json";$fixture="$env:USERPROFILE\OttrinFixtures"
$result=@{status='running';phase='prepare';scope='Native NSIS current-source global lock and fresh lifecycle; ordinary interactive user'}
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
 Assert ($env:USERNAME-eq'OttrinDesktopQA') 'Wrong account';Assert (-not(Test-Path $install)) 'Unexpected installation';$prefHash=(Get-FileHash $pref).Hash;$sourceHash=(Get-FileHash "$fixture\upgrade-source.txt").Hash
 $mutex=New-Object Threading.Mutex($false,'Global\OttrinInstaller')
 try{$out=Run "$b\locked-setup.exe" '/S' $false;Assert ($out.exit-eq1) 'Concurrent installer accepted';Assert (-not(Test-Path $install)) 'Lock refusal wrote payload';Assert (-not(Test-Path "$env:LOCALAPPDATA\ottrin\installer-index-transaction.json")) 'Lock refusal wrote journal';$result.lock_refusal_exit=$out.exit}finally{$mutex.Dispose()}
 Phase 'current-source-fresh';$null=Run "$b\locked-setup.exe" '/S';$id=[Security.Principal.WindowsIdentity]::GetCurrent();$s=New-Object -ComObject Schedule.Service;$s.Connect();$folder=$s.GetFolder('\');$taskName='Ottrin Index '+$id.User.Value;$result.live=Live;$null=Run $app 'index update --json';Check-Data
 $manifest=Get-Content "$b\new-payload.json" -Raw|ConvertFrom-Json;foreach($e in $manifest.PSObject.Properties){Assert ((Get-FileHash (Join-Path $install $e.Name)).Hash.ToLower()-eq$e.Value) "Payload differs: $($e.Name)"};Assert (-not(Test-Path "$env:LOCALAPPDATA\ottrin\installer-index-transaction.json")) 'Committed journal remains'
 Phase 'current-source-uninstall';$null=Run "$install\Uninstall.exe" '/S';$deadline=[DateTime]::UtcNow.AddSeconds(45);while((Test-Path $install)-and[DateTime]::UtcNow-lt$deadline){Start-Sleep -Milliseconds 200};Assert (-not(Test-Path $install)) 'Uninstall incomplete';Assert (-not(Test-Path "$env:LOCALAPPDATA\ottrin\search.db")) 'Index remains';Assert ((Get-FileHash $pref).Hash-eq$prefHash) 'Preferences changed';Assert ((Get-FileHash "$fixture\upgrade-source.txt").Hash-eq$sourceHash) 'Source changed';$result.installer_sha256=(Get-FileHash "$b\locked-setup.exe").Hash.ToLower();$result.status='pass';Phase 'uninstalled'
}catch{$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally{$result|ConvertTo-Json -Depth 12|Set-Content "$r\nsis-lock-result.json"}
if($result.status-ne'pass'){exit 1}
