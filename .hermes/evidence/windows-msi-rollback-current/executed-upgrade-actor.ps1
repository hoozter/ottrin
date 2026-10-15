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
 Assert ($env:USERNAME-eq'OttrinDesktopQA') 'Wrong account';$id=[Security.Principal.WindowsIdentity]::GetCurrent();Assert (-not(New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Elevated actor';Assert (-not(Test-Path $install)) 'Unexpected installation'
 New-Item -ItemType Directory $fixture,(Split-Path $pref) -Force|Out-Null;[IO.File]::WriteAllText("$fixture\upgrade-source.txt",'preserve this source')
 $c=Get-Content "$b\config-template.json" -Raw|ConvertFrom-Json;$c.theme='Dark';$c.default_view_mode='Miller';$c.bookmarks=@();$c.last_session_dirs=@();$c.search.include_roots=@($fixture);$c.search.exclude_roots=@();$c.search.refresh_interval_hours=0;$c.search.use_system_locate=$false;$c.search.manage_locate_db=$false;[IO.File]::WriteAllText($pref,($c|ConvertTo-Json -Depth 30),(New-Object Text.UTF8Encoding($false)));$prefHash=(Get-FileHash $pref).Hash;$sourceHash=(Get-FileHash "$fixture\upgrade-source.txt").Hash
 $s=New-Object -ComObject Schedule.Service;$s.Connect();$folder=$s.GetFolder('\');$taskName='Ottrin Index '+$id.User.Value;$i=New-Object -ComObject WindowsInstaller.Installer
 function ProductCode($path){$db=$script:i.OpenDatabase($path,0);$v=$db.OpenView("SELECT ``Value`` FROM ``Property`` WHERE ``Property``='ProductCode'");$null=$v.Execute();$code=$v.Fetch().StringData(1);$null=$v.Close();return $code}
 function MSI($arguments,$name){$p=Start-Process msiexec.exe -ArgumentList "$arguments /qn /norestart /l*v $script:r\$name.log" -PassThru;$null=$p.Handle;Assert ($p.WaitForExit(180000)) "MSI still live $($p.Id)";$exit=$p.ExitCode;$p.Dispose();Assert ($exit-eq0) "MSI $name returned $exit";return $exit}
 $old=ProductCode "$b\baseline.msi";$new=ProductCode "$b\current.msi";$result.old_product=$old;$result.new_product=$new;$result.installer_sha256=(Get-FileHash "$b\current.msi").Hash.ToLower()
 Phase 'baseline-install';$result.baseline_exit=MSI "/i $b\baseline.msi" 'baseline-install';$null=Run $app 'index update --json';Check-Data;$result.before=Live;$security=(Task).GetSecurityDescriptor(7);$created=[IO.File]::GetCreationTimeUtc("$env:LOCALAPPDATA\ottrin\search.db").Ticks
 Phase 'exact-running-upgrade';$result.upgrade_exit=MSI "/i $b\current.msi" 'running-upgrade';Assert ($i.ProductState($old)-eq-1-and$i.ProductState($new)-eq5) 'Upgrade registrations wrong';$result.after=Live;Assert ($result.after.pid-ne$result.before.pid) 'Old daemon survived';Assert ((Task).Enabled-and(Task).GetSecurityDescriptor(7)-eq$security) 'Task state/security changed';Assert ([IO.File]::GetCreationTimeUtc("$env:LOCALAPPDATA\ottrin\search.db").Ticks-eq$created) 'Database replaced'
 $manifest=Get-Content "$b\payload.json" -Raw|ConvertFrom-Json
 function Payload {foreach($e in $script:manifest.PSObject.Properties){Assert ((Get-FileHash (Join-Path $script:install $e.Name)).Hash.ToLower()-eq$e.Value) "Payload differs: $($e.Name)"};Assert (@(Get-ChildItem $script:install -Recurse -File).Count-eq@($script:manifest.PSObject.Properties).Count) 'Extra payload files';Check-Data}
 Payload;$result.payload_files=@($manifest.PSObject.Properties).Count;$result.repair_cases=@()
 foreach($case in @(@{enabled=$true;running=$true},@{enabled=$false;running=$true},@{enabled=$true;running=$false},@{enabled=$false;running=$false})){
  $t=Task;$t.Enabled=$case.enabled;if(-not$case.running){$null=Run "$install\bin\ottrin-indexd.exe" '--stop'}
  $name="repair-$($case.enabled)-$($case.running)";Phase $name;$exit=MSI "/i $b\current.msi REINSTALL=ALL REINSTALLMODE=amus" $name;Assert ($i.ProductState($new)-eq5) 'Repair registration wrong';Assert ((Task).Enabled-eq$case.enabled) 'Repair changed login intent';Assert (((Task).GetInstances(0).Count-gt0)-eq$case.running) 'Repair changed runtime intent';Assert ((Task).GetSecurityDescriptor(7)-eq$security) 'Repair changed security';Payload;$result.repair_cases+=@{enabled=$case.enabled;running=$case.running;exit=$exit;status='pass'}
 }
 $t=Task;$t.Enabled=$true;$null=Run $app 'index service-install';$result.ready=Live;$result.status='pass';Phase 'ready-for-fault-tests'
}catch{$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally{$result|ConvertTo-Json -Depth 12|Set-Content "$r\upgrade-result.json"}
if($result.status-ne'pass'){exit 1}
