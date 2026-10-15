$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$b='C:\OttrinDesktopQA';$r="$b\results";$controller="$b\controller.exe";$install="$env:LOCALAPPDATA\Programs\Ottrin";$app="$install\bin\ottrin.exe";$pref="$env:APPDATA\ottrin\config.json";$fixture="$env:USERPROFILE\OttrinFixtures"
$result=@{status='running';phase='prepare';scope='Clean fcf00aa MSI native fresh ordinary-user lifecycle'}
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
 Assert ($env:USERNAME-eq'OttrinDesktopQA') 'Wrong account';Assert (-not(Test-Path $install)) 'Unexpected installation';$id=[Security.Principal.WindowsIdentity]::GetCurrent();Assert (-not(New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Elevated actor'
 $prefHash=(Get-FileHash $pref).Hash;$sourceHash=(Get-FileHash "$fixture\upgrade-source.txt").Hash;$s=New-Object -ComObject Schedule.Service;$s.Connect();$folder=$s.GetFolder('\');$taskName='Ottrin Index '+$id.User.Value
 $i=New-Object -ComObject WindowsInstaller.Installer;$db=$i.OpenDatabase("$b\current.msi",0);$v=$db.OpenView("SELECT ``Value`` FROM ``Property`` WHERE ``Property``='ProductCode'");$null=$v.Execute();$code=$v.Fetch().StringData(1);$null=$v.Close();$result.product=$code
 Phase 'final-msi-install';$p=Start-Process msiexec.exe -ArgumentList "/i $b\current.msi /qn /norestart /l*v $r\final-msi-install.log" -PassThru;$null=$p.Handle;Assert ($p.WaitForExit(180000)) "Live MSI install $($p.Id)";$result.install_exit=$p.ExitCode;$p.Dispose();Assert ($result.install_exit-eq0-and$i.ProductState($code)-eq5) 'Fresh MSI install failed'
 $result.live=Live;$manifest=Get-Content "$b\new-payload.json" -Raw|ConvertFrom-Json;foreach($e in $manifest.PSObject.Properties){Assert ((Get-FileHash (Join-Path $install $e.Name)).Hash.ToLower()-eq$e.Value) "Payload differs: $($e.Name)"};$result.payload_files=@($manifest.PSObject.Properties).Count;Assert (@(Get-ChildItem $install -Recurse -File).Count-eq$result.payload_files) 'Unexpected installed files';$null=Run $app 'index update --json';Check-Data
 Phase 'final-msi-uninstall';$p=Start-Process msiexec.exe -ArgumentList "/x $code /qn /norestart /l*v $r\final-msi-uninstall.log" -PassThru;$null=$p.Handle;Assert ($p.WaitForExit(120000)) "Live MSI uninstall $($p.Id)";$result.uninstall_exit=$p.ExitCode;$p.Dispose();Assert ($result.uninstall_exit-eq0-and$i.ProductState($code)-eq-1) 'Fresh MSI uninstall failed';Assert (-not(Test-Path $install)) 'Installed files remain';Assert (-not(Test-Path "$env:LOCALAPPDATA\ottrin\search.db")) 'Index remains';Assert ((Get-FileHash $pref).Hash-eq$prefHash) 'Preferences changed';Assert ((Get-FileHash "$fixture\upgrade-source.txt").Hash-eq$sourceHash) 'Source changed';$absent=$false;try{$null=Task}catch{if($_.Exception.HResult-eq-2147024894){$absent=$true}else{throw}};Assert $absent 'Task remains'
 $result.installer_sha256=(Get-FileHash "$b\current.msi").Hash.ToLower();$result.status='pass';Phase 'uninstalled'
}catch{$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally{$result|ConvertTo-Json -Depth 12|Set-Content "$r\final-msi-result.json"}
if($result.status-ne'pass'){exit 1}
