$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$b='C:\OttrinDesktopQA';$r="$b\results";$controller="$b\controller.exe";$install="$env:LOCALAPPDATA\Programs\Ottrin";$app="$install\bin\ottrin.exe";$pref="$env:APPDATA\ottrin\config.json";$fixture="$env:USERPROFILE\OttrinFixtures"
$result=@{status='running';phase='prepare';scope='Native file-only per-user controls: one text file, HKCU key path, standard Type19 failure after executed file operations, no Ottrin application/custom executable'}
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
 Assert ($env:USERNAME-eq'OttrinDesktopQA') 'Wrong account';$id=[Security.Principal.WindowsIdentity]::GetCurrent();Assert (-not(New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Elevated actor';$i=New-Object -ComObject WindowsInstaller.Installer;$result.cases=@()
 function Code($path){$db=$script:i.OpenDatabase($path,0);$v=$db.OpenView("SELECT ``Value`` FROM ``Property`` WHERE ``Property``='ProductCode'");$null=$v.Execute();$value=$v.Fetch().StringData(1);$null=$v.Close();return $value}
 function MSI($arguments,$name){$p=Start-Process msiexec.exe -ArgumentList "$arguments /qn /norestart /l*v $script:r\$name.log" -PassThru;$null=$p.Handle;Assert ($p.WaitForExit(120000)) "MSI remains live $($p.Id)";$exit=$p.ExitCode;$p.Dispose();return $exit}
 foreach($kind in @('install','uninstall')){
  Phase "control-$kind";$path="$b\control-$kind.msi";$code=Code $path;$payload="$env:LOCALAPPDATA\OttrinRollbackControl$kind\control.txt";Assert (-not(Test-Path $payload)) 'Control payload already exists';$case=@{kind=$kind;product=$code;msi_sha256=(Get-FileHash $path).Hash.ToLower();state_before=$i.ProductState($code)}
  if($kind-eq'uninstall'){$case.install_exit=MSI "/i $path" 'control-good-install';Assert ($case.install_exit-eq0-and(Test-Path $payload)-and$i.ProductState($code)-eq5) 'Control baseline install failed';$case.failure_exit=MSI "/x $code OTTRIN_QA_FAIL=1" 'control-failed-uninstall'}else{$case.failure_exit=MSI "/i $path OTTRIN_QA_FAIL=1" 'control-failed-install'}
  Assert ($case.failure_exit-eq1603) 'Control failure not exercised';$case.state_after_failure=$i.ProductState($code);$case.payload_after_failure=(Test-Path $payload)
  $log=Get-Content "$r\control-failed-$kind.log" -Raw;$op=if($kind-eq'install'){'Executing op: FileCopy'}else{'Executing op: FileRemove'};$opIndex=$log.IndexOf($op);$failureIndex=$log.IndexOf('Doing action: DeliberateFailure');Assert ($opIndex-ge0-and$failureIndex-gt$opIndex) 'Failure happened before actual file execution';$case.actual_file_operation_before_failure=$true;$case.skipped_rollback_errors=([regex]::Matches($log,'Error in rollback skipped')).Count
  # Use normal supported repair/uninstall APIs, never manual registry changes.
  $case.repair_exit=MSI "/i $path REINSTALL=ALL REINSTALLMODE=amus" "control-$kind-repair";$case.state_after_repair=$i.ProductState($code);$case.cleanup_exit=MSI "/x $code" "control-$kind-cleanup";$case.payload_after_cleanup=(Test-Path $payload);$case.state_after_cleanup=$i.ProductState($code);$result.cases+=$case
 }
 $result.status='observed';Phase 'observed-file-only-controls'
}catch{$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally{$result|ConvertTo-Json -Depth 12|Set-Content -Encoding UTF8 "$r\control-result.json"}
if($result.status-ne'observed'){exit 1}
