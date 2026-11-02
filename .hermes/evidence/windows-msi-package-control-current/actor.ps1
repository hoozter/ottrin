param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExpectedSid)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$out=Join-Path $Root 'results'
$result=@{status='running';scope='Exact ordinary-user Ottrin MSI installation, genuine locked-cache refusal and supported retry; no patched installer or synthetic MSI failure'}
$lock=$null;$app=$null
function Require($value,$message){if(-not$value){throw $message}}
function Save {$result|ConvertTo-Json -Depth 20|Set-Content -Encoding UTF8 (Join-Path $out 'RESULT.json')}
function Phase($name){$result.phase=$name;Save}
function Run([string]$program,[string]$arguments,[string]$name){
 $start=[Diagnostics.ProcessStartInfo]::new($program,$arguments)
 $start.UseShellExecute=$false;$start.RedirectStandardOutput=$true;$start.RedirectStandardError=$true;$start.CreateNoWindow=$true
 $process=[Diagnostics.Process]::Start($start)
 try{
  $stdout=$process.StandardOutput.ReadToEndAsync();$stderr=$process.StandardError.ReadToEndAsync()
  Require ($process.WaitForExit(180000)) ('Owned child timed out: '+$name)
  Require ($stdout.Wait(5000)-and$stderr.Wait(5000)) ('Owned child output remained open: '+$name)
  $value=@{exit=$process.ExitCode;stdout=$stdout.Result;stderr=$stderr.Result}
  $value|ConvertTo-Json -Depth 4|Set-Content -Encoding UTF8 (Join-Path $out ($name+'-process.json'))
  return $value
 }finally{
  if(-not$process.HasExited){& "$env:SystemRoot\System32\taskkill.exe" /PID $process.Id /T /F|Out-Null;$null=$process.WaitForExit(5000)}
  $process.Dispose()
 }
}
function MSI([string]$arguments,[string]$name){
 $log=Join-Path $out ($name+'.log')
 return Run "$env:SystemRoot\System32\msiexec.exe" ($arguments+' /qn /norestart /l*v "'+$log+'"') $name
}
function InstalledTask {
 $matches=@($folder.GetTasks(1)|Where-Object {
  $task=$_
  $task.Name.StartsWith('Ottrin Index')-and@($task.Definition.Actions|Where-Object {([string]$_.Path).Trim('"')-ieq$daemon}).Count-eq1
 })
 Require ($matches.Count-eq1) 'Expected one task for the exact installed index daemon'
 $owner=$matches[0].Definition.Principal.UserId
 if(-not$owner.StartsWith('S-')){$owner=([Security.Principal.NTAccount]::new($owner)).Translate([Security.Principal.SecurityIdentifier]).Value}
 Require ($owner-eq$ExpectedSid) 'Installed task belongs to another account'
 return $matches[0]
}
function Live {
 $task=InstalledTask;$instances=$task.GetInstances(0)
 Require ($instances.Count-eq1) 'Expected one actual installed task instance'
 $processes=@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue|Where-Object {$_.Path-ieq$daemon})
 Require ($processes.Count-eq1) 'Expected one actual installed daemon process'
 return @{pid=$processes[0].Id;instance=$instances.Item(1).InstanceGuid}
}
function FileId([string]$path){
 $file=[IO.File]::Open($path,'Open','Read',([IO.FileShare]::ReadWrite-bor[IO.FileShare]::Delete))
 try{
  $info=New-Object OttrinOwnedFileId+Info
  if(-not[OttrinOwnedFileId]::GetFileInformationByHandle($file.SafeFileHandle.DangerousGetHandle(),[ref]$info)){throw 'Native owned database file identity could not be read'}
  return $info.volume.ToString('X8')+':'+$info.indexHigh.ToString('X8')+$info.indexLow.ToString('X8')
 }finally{$file.Dispose()}
}
try{
 $identity=[Security.Principal.WindowsIdentity]::GetCurrent()
 Require ($identity.User.Value-eq$ExpectedSid) 'Wrong application actor SID'
 Require (-not([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Ordinary token required'
 $session=(Get-Process -Id $PID).SessionId;Require ($session-gt0) 'Actual interactive session required'
 $profile=[Environment]::GetFolderPath([Environment+SpecialFolder]::UserProfile)
 Require ((Split-Path $profile -Leaf)-like($identity.Name.Split('\')[-1]+'*')) 'Owned user profile was not loaded'
 [Environment]::SetEnvironmentVariable('HOME',$profile,'Process')
 $result.identity=@{sid=$ExpectedSid;profile=$profile;administrator=$false;session=$session}
 Phase 'waiting for owned desktop'
 $deadline=[DateTime]::UtcNow.AddMinutes(8);$readyPath=Join-Path $out 'DESKTOP_READY.json'
 while(-not(Test-Path $readyPath)){Require ([DateTime]::UtcNow-lt$deadline) 'Owned desktop was not observed';Start-Sleep -Milliseconds 250}
 $ready=Get-Content $readyPath -Raw|ConvertFrom-Json;$owner=Get-Content 'C:\OttrinMsiCleanClientResults\OWNER.json' -Raw|ConvertFrom-Json
 Require ($ready.sid-eq$ExpectedSid-and$ready.nonce-eq$owner.nonce) 'Desktop signal belongs to another fixture'
 $result.desktop_ready=$true
 Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class OttrinOwnedFileId {
 [StructLayout(LayoutKind.Sequential)] public struct Info {
  public uint attr,creationLo,creationHi,accessLo,accessHi,writeLo,writeHi,volume,sizeHigh,sizeLow,links,indexHigh,indexLow;
 }
 [DllImport("kernel32.dll",SetLastError=true)] public static extern bool GetFileInformationByHandle(IntPtr handle,out Info info);
}
'@
 $input=Get-Content (Join-Path $Root 'application-input.json') -Raw|ConvertFrom-Json
 $msi=Join-Path $Root 'application.msi';Require ((Get-FileHash $msi).Hash.ToLowerInvariant()-eq$input.sha256) 'Actual package identity mismatch'
 $result.package=$input
 $install=Join-Path $env:LOCALAPPDATA 'Programs\Ottrin';$app=Join-Path $install 'bin\ottrin.exe';$daemon=Join-Path $install 'bin\ottrin-indexd.exe'
 Require (-not(Test-Path $install)) 'An existing installation must not be touched'
 $fixture=Join-Path $profile 'OttrinOwnedMsiFiles';Require (-not(Test-Path $fixture)) 'Existing fixture must not be touched';$null=New-Item -ItemType Directory $fixture
 $source=Join-Path $fixture 'rollback-proof.txt';[IO.File]::WriteAllText($source,'Ottrin MSI source bytes remain unchanged.');$sourceHash=(Get-FileHash $source).Hash
 $config=Get-Content (Join-Path $Root 'config-template.json') -Raw|ConvertFrom-Json
 $config.bookmarks=@();$config.theme='Dark';$config.default_view_mode='Miller';$config.installed_office_previews_enabled=$false
 $config.search.include_roots=@($fixture);$config.search.exclude_roots=@();$config.search.content_indexing_enabled=$false
 $pref=Join-Path $env:APPDATA 'ottrin\config.json';Require (-not(Test-Path $pref)) 'Existing preferences must not be replaced'
 $null=[IO.Directory]::CreateDirectory((Split-Path $pref));[IO.File]::WriteAllText($pref,($config|ConvertTo-Json -Depth 60),[Text.UTF8Encoding]::new($false));$prefHash=(Get-FileHash $pref).Hash
 $installer=New-Object -ComObject WindowsInstaller.Installer;$db=$installer.OpenDatabase($msi,0);$view=$db.OpenView("SELECT ``Value`` FROM ``Property`` WHERE ``Property``='ProductCode'");$null=$view.Execute();$record=$view.Fetch();$code=$record.StringData(1);$view.Close()
 foreach($object in @($record,$view,$db)){$null=[Runtime.InteropServices.Marshal]::FinalReleaseComObject($object)}
 $result.product=$code;Require ($installer.ProductState($code)-eq-1) 'Candidate product is not fresh'
 Phase 'fresh actual MSI install';$run=MSI ('/i "'+$msi+'"') 'install';$result.install_exit=$run.exit
 Require ($run.exit-eq0-and$installer.ProductState($code)-eq5) 'Fresh actual installation failed'
 $manifest=Get-Content (Join-Path $Root 'payload.json') -Raw|ConvertFrom-Json
 foreach($entry in $manifest.PSObject.Properties){Require ((Get-FileHash (Join-Path $install $entry.Name)).Hash.ToLowerInvariant()-eq$entry.Value) ('Installed payload mismatch: '+$entry.Name)}
 $result.installed_payload_files=$manifest.PSObject.Properties.Count
 Require ((Get-FileHash $pref).Hash-eq$prefHash) 'Fresh installer changed preferences'
 $run=Run $app 'index update --json' 'initial-index';Require ($run.exit-eq0) 'Installed index initialization failed'
 $service=New-Object -ComObject Schedule.Service;$service.Connect();$folder=$service.GetFolder('\')
 $before=Live;$task=InstalledTask;$xml=$task.Xml;$security=$task.GetSecurityDescriptor(7);$saved=Join-Path $env:LOCALAPPDATA 'ottrin\search.db';$dbIdentity=FileId $saved
 $result.before=$before;$result.database_file_id_before=$dbIdentity
 $cache=Join-Path $env:LOCALAPPDATA 'ottrin\preview_cache';$null=[IO.Directory]::CreateDirectory($cache);$locked=Join-Path $cache 'owned-delete-denial';[IO.File]::WriteAllText($locked,'Owned refusal fixture')
 $lock=[IO.File]::Open($locked,'Open','Read',[IO.FileShare]::Read)
 try{Phase 'actual locked-cache uninstall';$run=MSI ("/x $code") 'failure';$result.failure_exit=$run.exit;Require ($run.exit-eq1603) 'Actual locked-cache refusal was not exercised'}finally{$lock.Dispose();$lock=$null;Remove-Item -LiteralPath $locked}
 $result.state_after_failure=$installer.ProductState($code);$after=Live;$task=InstalledTask
 $result.after=$after;$result.daemon_instance_unchanged=($before.pid-eq$after.pid-and$before.instance-eq$after.instance)
 $result.task_xml_unchanged=($task.Xml-ceq$xml);$result.task_security_unchanged=($task.GetSecurityDescriptor(7)-ceq$security)
 $result.payload_intact=$true
 foreach($entry in $manifest.PSObject.Properties){if((Get-FileHash (Join-Path $install $entry.Name)).Hash.ToLowerInvariant()-ne$entry.Value){$result.payload_intact=$false}}
 $result.preferences_preserved=((Get-FileHash $pref).Hash-eq$prefHash);$result.source_preserved=((Get-FileHash $source).Hash-eq$sourceHash)
 $result.database_file_id_after=FileId $saved;$result.database_identity_preserved=($result.database_file_id_after-eq$dbIdentity)
 $run=Run $app 'search rollback-proof --json' 'query-after-refusal';$result.index_query_preserved=($run.exit-eq0-and($run.stdout|ConvertFrom-Json).total-eq1)
 $log=Get-Content (Join-Path $out 'failure.log') -Raw;$result.raw_skipped_rollback_lines=[regex]::Matches($log,'Error in rollback skipped').Count
 Phase 'supported ordinary uninstall retry';$run=MSI ("/x $code") 'retry';$result.retry_exit=$run.exit;$result.state_after_retry=$installer.ProductState($code);$result.payload_after_retry=Test-Path $install
 $result.preferences_after_retry=((Get-FileHash $pref).Hash-eq$prefHash);$result.source_after_retry=((Get-FileHash $source).Hash-eq$sourceHash)
 $result.state_gate_passed=($result.state_after_failure-eq5-and$result.retry_exit-eq0-and$result.state_after_retry-eq-1-and-not$result.payload_after_retry-and$result.daemon_instance_unchanged-and$result.task_xml_unchanged-and$result.task_security_unchanged-and$result.payload_intact-and$result.preferences_preserved-and$result.source_preserved-and$result.database_identity_preserved-and$result.index_query_preserved-and$result.preferences_after_retry-and$result.source_after_retry)
 $result.status='observed';Phase 'actual MSI observation complete; inspect gate and native log'
}catch{$result.status='failed application fixture';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally{
 if($lock){$lock.Dispose()}
 if($app-and(Test-Path $app)){
  try{$cleanup=Run $app 'index service-remove' 'fixture-service-cleanup';$result.service_cleanup_exit=$cleanup.exit}catch{$result.service_cleanup_error=$_.Exception.Message}
 }
 Save
}
if($result.status-ne'observed'){exit 1}
