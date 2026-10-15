$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
if($env:USERNAME-ne'OttrinDesktopQA'){throw 'Wrong account'}
$b='C:\OttrinDesktopQA';$i=New-Object -ComObject WindowsInstaller.Installer
foreach($name in @('current.msi','baseline.msi')){
 $db=$i.OpenDatabase("$b\$name",0);$v=$db.OpenView("SELECT ``Value`` FROM ``Property`` WHERE ``Property``='ProductCode'");$null=$v.Execute();$code=$v.Fetch().StringData(1);$null=$v.Close()
 if($i.ProductState($code)-gt0){$p=Start-Process msiexec.exe -ArgumentList "/x $code /qn /norestart /l*v $b\results\fixture-recovery.log" -PassThru;$null=$p.Handle;if(-not$p.WaitForExit(120000)){throw "Recovery process live: $($p.Id)"};if($p.ExitCode-ne0){throw "Recovery uninstall exit $($p.ExitCode)"}}
 if($i.ProductState($code)-ne-1){throw 'Product registration remains'}
}
if(Test-Path "$env:LOCALAPPDATA\Programs\Ottrin"){throw 'App remains'}
@{status='pass';operation='Normal per-user uninstall after test-script return-value failure';profile=$env:USERNAME}|ConvertTo-Json|Set-Content "$b\results\fixture-recovery.json"
