$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$b='C:\OttrinMsiControlQA';$r="$b\results";$result=@{status='running';phase='start';cases=@()}
function Assert($value,$message){if(-not$value){throw $message}}
function Phase($name){$result.phase=$name;[IO.File]::WriteAllText("$r\PROGRESS.json",($result|ConvertTo-Json -Depth 20))}
function RunMsi($arguments){
 $p=Start-Process msiexec.exe -ArgumentList $arguments -PassThru
 try{Assert ($p.WaitForExit(180000)) 'Bounded owned MSI process timed out';return $p.ExitCode}
 finally{if(-not$p.HasExited){Stop-Process -Id $p.Id -Force}}
}
function ProductSecurity($name){
 $out=@();$base='Registry::HKEY_CURRENT_USER\Software\Microsoft\Installer\Products'
 if(Test-Path $base){foreach($key in Get-ChildItem $base){$value=Get-ItemProperty $key.PSPath;if($value.ProductName-eq$name){$acl=Get-Acl $key.PSPath;$out+=@{path=$key.Name;owner=$acl.Owner;sddl=$acl.Sddl}}}}
 return $out
}
try{
 Assert ($env:USERNAME-eq'OttrinMsiControlQA') 'Wrong owned account'
 $id=[Security.Principal.WindowsIdentity]::GetCurrent();$admin=(New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator);Assert (-not$admin) 'Ordinary control must be unelevated'
 $result.identity=@{sid=$id.User.Value;owner=$id.Owner.Value;administrator=$admin;session=[Diagnostics.Process]::GetCurrentProcess().SessionId}
 $result.os=(Get-CimInstance Win32_OperatingSystem|Select-Object Caption,Version,BuildNumber);$result.msi_version=(Get-Item "$env:SystemRoot\System32\msi.dll").VersionInfo.FileVersion
 $installer=New-Object -ComObject WindowsInstaller.Installer
 foreach($case in @('plain','icon')){
  $msi="$b\$case.msi";$target="$env:LOCALAPPDATA\OttrinMsiControlPayload-$case\control.txt";$name="Ottrin isolated MSI engine control $case";$c=@{case=$case;msi_sha256=(Get-FileHash $msi).Hash.ToLowerInvariant()}
  $db=$installer.OpenDatabase($msi,0);$view=$db.OpenView("SELECT ``Value`` FROM ``Property`` WHERE ``Property``='ProductCode'");$view.Execute();$code=$view.Fetch().StringData(1);$view.Close();$c.product=$code
  Assert (-not(Test-Path $target)) 'Owned control payload already exists'
  Phase "$case-positive-install";$c.positive_install_exit=RunMsi "/i $msi /qn /norestart /l*v $r\$case-positive-install.log";Assert ($c.positive_install_exit-eq0-and$installer.ProductState($code)-eq5-and(Test-Path $target)) 'Positive control installation failed'
  $c.file_sha256=(Get-FileHash $target).Hash.ToLowerInvariant()
  Phase "$case-positive-uninstall";$c.positive_uninstall_exit=RunMsi "/x $code /qn /norestart /l*v $r\$case-positive-uninstall.log";$c.positive_state_after=$installer.ProductState($code);Assert ($c.positive_uninstall_exit-eq0-and$c.positive_state_after-eq-1-and -not(Test-Path $target)) 'Positive control removal failed'
  Phase "$case-reinstall";$c.reinstall_exit=RunMsi "/i $msi /qn /norestart /l*v $r\$case-reinstall.log";Assert ($c.reinstall_exit-eq0-and$installer.ProductState($code)-eq5) 'Control reinstall failed'
  $c.registration_before=ProductSecurity $name
  Phase "$case-injected-failure";$c.failure_exit=RunMsi "/x $code FAILUNINSTALL=1 /qn /norestart /l*v $r\$case-failure.log";Assert ($c.failure_exit-eq1603) 'Deferred refusal did not execute'
  $c.state_after_failure=$installer.ProductState($code);$c.file_preserved=(Test-Path $target)-and((Get-FileHash $target).Hash.ToLowerInvariant()-eq$c.file_sha256);$c.registration_after=ProductSecurity $name
  $log=Get-Content "$r\$case-failure.log" -Raw;$c.rollback_errors=[regex]::Matches($log,'Error in rollback skipped').Count;$c.invalid_owner_errors=[regex]::Matches($log,'3: 1307').Count;$c.deferred_failure_exercised=$log-match'CustomAction FailRemoval returned actual error code 1'
  Assert $c.deferred_failure_exercised 'Actual deferred failure was not observed'
  Phase "$case-normal-retry";$c.retry_exit=RunMsi "/x $code /qn /norestart /l*v $r\$case-retry.log";$c.state_after_retry=$installer.ProductState($code);$c.file_after_retry=Test-Path $target
  $c.status=if($c.state_after_failure-eq5-and$c.file_preserved-and$c.rollback_errors-eq0-and$c.retry_exit-eq0-and$c.state_after_retry-eq-1-and-not$c.file_after_retry){'rollback-and-retry-pass'}else{'rollback-or-retry-failed'}
  $result.cases+=,$c
 }
 $result.status='observed';Phase 'complete'
}catch{$result.status='failed';$result.error=$_.Exception.ToString()}
finally{[IO.File]::WriteAllText("$r\RESULT.json",($result|ConvertTo-Json -Depth 30));[IO.File]::WriteAllText("$r\COMPLETE",'complete')}
