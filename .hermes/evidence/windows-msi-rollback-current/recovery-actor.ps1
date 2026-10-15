$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$b='C:\OttrinDesktopQA';$r="$b\results";$controller="$b\controller.exe";$install="$env:LOCALAPPDATA\Programs\Ottrin";$app="$install\bin\ottrin.exe";$pref="$env:APPDATA\ottrin\config.json";$fixture="$env:USERPROFILE\OttrinFixtures"
$result=@{status='running';phase='prepare';scope='Supported ordinary-user MSI APIs after verified registration rollback failure; no manual registry/ACL/policy changes'}
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
 Assert ($env:USERNAME-eq'OttrinDesktopQA') 'Wrong account';$id=[Security.Principal.WindowsIdentity]::GetCurrent();Assert (-not(New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Elevated actor';$i=New-Object -ComObject WindowsInstaller.Installer
 function MSI($arguments,$name){$p=Start-Process msiexec.exe -ArgumentList "$arguments /qn /norestart /l*v $script:r\$name.log" -PassThru;$null=$p.Handle;Assert ($p.WaitForExit(180000)) "MSI remains live $($p.Id)";$exit=$p.ExitCode;$p.Dispose();return $exit}
 $u=Get-Content "$r\upgrade-result.json" -Raw|ConvertFrom-Json;$code=$u.new_product;$result.product=$code;$result.state_before=$i.ProductState($code);Assert ($result.state_before-eq1) 'No advertised failure state'
 Phase 'supported-addlocal-recovery';$result.repair_exit=MSI "/i $b\current.msi ADDLOCAL=ALL REINSTALLMODE=amus" 'production-addlocal-repair';$result.state_after_repair=$i.ProductState($code);$result.uninstall_exit=MSI "/x $code" 'production-recovery-uninstall';$result.state_after_uninstall=$i.ProductState($code);$result.app_files_remaining=(Test-Path $install)
 Phase 'control-fresh-reinstall-recovery';$result.control_install_exit=MSI "/i $b\control-uninstall.msi" 'control-fresh-reinstall';$result.control_uninstall_exit=MSI "/x $b\control-uninstall.msi" 'control-recovery-uninstall';$result.control_file_remaining=(Test-Path "$env:LOCALAPPDATA\OttrinRollbackControlUninstall\control.txt")
 if(Test-Path $app){$cleanup=Run $app '--uninstall';$result.application_cleanup_exit=$cleanup.exit}
 $result.status='observed';Phase 'observed-supported-recovery'
}catch{$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally{$result|ConvertTo-Json -Depth 12|Set-Content -Encoding UTF8 "$r\recovery-result.json"}
if($result.status-ne'observed'){exit 1}
