param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExpectedSid)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$out=Join-Path $Root 'results'
$result=@{status='running';scope='Fresh native ordinary-user file-only MSI rollback after executed copy/removal; no Ottrin executable or application custom action';cases=@()}
function Require($ok,$message){if(-not$ok){throw $message}}
function Save {$result|ConvertTo-Json -Depth 12|Set-Content -Encoding UTF8 (Join-Path $out 'RESULT.json')}
function RegistrySnapshot([string]$Product,[string]$Name){
 # Read security descriptors and request individual access masks only. Never
 # create keys, set values, change permissions or repair the installer state.
 $hex=([Guid]$Product).ToString('N').ToUpperInvariant();$packed=''
 foreach($range in @(@(0,7),@(8,11),@(12,15))){for($j=$range[1];$j-ge$range[0];$j--){$packed+=$hex[$j]}}
 for($j=16;$j-lt32;$j+=2){$packed+=$hex[$j+1];$packed+=$hex[$j]}
 $base=$ExpectedSid+'\Software\Microsoft\Installer'
 $paths=@($ExpectedSid;($ExpectedSid+'\Software');($ExpectedSid+'\Software\Microsoft');$base;($base+'\Products');($base+'\Products\'+$packed);($base+'\Products\'+$packed+'\SourceList'))
 $keys=@()
 foreach($path in $paths){
  $entry=@{path='HKEY_USERS\'+$path;access=@{}}
  $securityKey=$null
  try{
   $securityKey=[Microsoft.Win32.Registry]::Users.OpenSubKey($path,[Microsoft.Win32.RegistryKeyPermissionCheck]::Default,[Security.AccessControl.RegistryRights]::ReadPermissions)
   if($null-eq$securityKey){$entry.acl_state='not found'}else{
    $sections=[Security.AccessControl.AccessControlSections]::Owner-bor[Security.AccessControl.AccessControlSections]::Group-bor[Security.AccessControl.AccessControlSections]::Access
    $acl=$securityKey.GetAccessControl($sections);$entry.owner=$acl.GetOwner([Security.Principal.SecurityIdentifier]).Value;$entry.sddl=$acl.GetSecurityDescriptorSddlForm($sections);$entry.protected=$acl.AreAccessRulesProtected
   }
  }catch{$entry.acl_error=$_.Exception.Message}finally{if($securityKey){$securityKey.Dispose()}}
  foreach($right in @('QueryValues','SetValue','CreateSubKey','ReadPermissions','Delete')){
   $key=$null
   try{$key=[Microsoft.Win32.Registry]::Users.OpenSubKey($path,[Microsoft.Win32.RegistryKeyPermissionCheck]::Default,[Security.AccessControl.RegistryRights]$right);$entry.access[$right]=if($null-eq$key){'not found'}else{'granted'}}catch{$entry.access[$right]=@{error=$_.Exception.Message;hresult=$_.Exception.HResult}}finally{if($key){$key.Dispose()}}
  }
  $keys+=$entry
 }
 @{sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value;product=$Product;packed_product=$packed;phase=$Name;scope='Read-only exact owned user installer registry security/access inspection';keys=$keys}|ConvertTo-Json -Depth 10|Set-Content -Encoding UTF8 (Join-Path $out ($Name+'-registry.json'))
}
function MSI([string]$arguments,[string]$name){
 $log=Join-Path $out ($name+'.log');$p=Start-Process msiexec.exe -ArgumentList ($arguments+' /qn /norestart /l*v "'+$log+'"') -PassThru;$null=$p.Handle
 $track=Join-Path $out 'active-msi.json';@{pid=$p.Id;started=$p.StartTime.ToUniversalTime().Ticks}|ConvertTo-Json|Set-Content -Encoding UTF8 $track
 try {if(-not$p.WaitForExit(120000)){$p.Kill();$null=$p.WaitForExit(5000);throw 'Owned MSI process timed out'};$p.WaitForExit();return $p.ExitCode}finally{if(-not$p.HasExited){$p.Kill();$null=$p.WaitForExit(5000)};$p.Dispose();Remove-Item $track -ErrorAction SilentlyContinue}
}
try {
 $id=[Security.Principal.WindowsIdentity]::GetCurrent();Require ($id.User.Value-eq$ExpectedSid) 'Wrong hosted actor SID';Require (-not([Security.Principal.WindowsPrincipal]::new($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Ordinary token required'
 $session=(Get-Process -Id $PID).SessionId;Require ($session-gt0) 'Real ordinary interactive session required';$result.session_id=$session
 $profilePath=[Environment]::GetFolderPath([Environment+SpecialFolder]::UserProfile);$account=$id.Name.Split('\')[-1];Require ((Split-Path $profilePath -Leaf)-like($account+'*')) 'Owned user profile was not loaded'
 foreach($pair in @(@('USERPROFILE',$profilePath),@('HOME',$profilePath),@('LOCALAPPDATA',(Join-Path $profilePath 'AppData\Local')),@('APPDATA',(Join-Path $profilePath 'AppData\Roaming')),@('TEMP',(Join-Path $profilePath 'AppData\Local\Temp')),@('TMP',(Join-Path $profilePath 'AppData\Local\Temp')))){[Environment]::SetEnvironmentVariable($pair[0],$pair[1],'Process');if(-not(Test-Path $pair[1])){New-Item -ItemType Directory $pair[1] -Force|Out-Null}}
 $result.identity=@{sid=$ExpectedSid;profile=$profilePath;administrator=$false};$result.windows=@{caption=(Get-CimInstance Win32_OperatingSystem).Caption;build=(Get-CimInstance Win32_OperatingSystem).BuildNumber;msi_version=(Get-Item "$env:SystemRoot\System32\msi.dll").VersionInfo.FileVersion}
 & "$env:SystemRoot\System32\whoami.exe" /all|Set-Content -Encoding UTF8 (Join-Path $out 'ordinary-token.txt');Require ($LASTEXITCODE-eq0) 'Token inspection failed'
 $i=New-Object -ComObject WindowsInstaller.Installer
 $result.phase='waiting for owned ordinary desktop';Save;$deadline=[DateTime]::UtcNow.AddMinutes(8);$readyPath=Join-Path $out 'DESKTOP_READY.json';while(-not(Test-Path $readyPath)){if([DateTime]::UtcNow-ge$deadline){throw 'Owned first-login desktop was not made ready'};Start-Sleep -Milliseconds 250};$ready=Get-Content $readyPath -Raw|ConvertFrom-Json;Require ($ready.sid-eq$ExpectedSid) 'Desktop readiness belongs to another fixture';$result.desktop_ready=$true;$result.phase='executing native MSI rollback control';Save
 foreach($kind in @('install','uninstall')) {
  $path=Join-Path $Root ("control-$kind.msi");$db=$i.OpenDatabase($path,0);$v=$db.OpenView("SELECT ``Value`` FROM ``Property`` WHERE ``Property``='ProductCode'");$null=$v.Execute();$record=$v.Fetch();$code=$record.StringData(1);$null=$v.Close();foreach($o in @($record,$v,$db)){$null=[Runtime.InteropServices.Marshal]::FinalReleaseComObject($o)}
  $payload=Join-Path $env:LOCALAPPDATA ("OttrinHostedRollback-$kind\control.txt");Require (-not(Test-Path $payload)) 'Foreign control payload retained';Require ($i.ProductState($code)-eq-1) 'Control already registered'
  $case=@{kind=$kind;product=$code;msi_sha256=(Get-FileHash $path).Hash;state_before=$i.ProductState($code)};$result.cases+=$case;Save
  RegistrySnapshot $code ($kind+'-before')
  if($kind-eq'uninstall'){$case.install_exit=MSI ('/i "'+$path+'"') 'baseline-uninstall';Require ($case.install_exit-eq0-and(Test-Path $payload)-and$i.ProductState($code)-eq5) 'Baseline install failed';RegistrySnapshot $code 'uninstall-baseline';$case.failure_exit=MSI ("/x $code OTTRIN_QA_FAIL=1") 'failure-uninstall'}else{$case.failure_exit=MSI ('/i "'+$path+'" OTTRIN_QA_FAIL=1') 'failure-install'}
  Require ($case.failure_exit-eq1603) 'Deliberate failure did not execute';$case.state_after_failure=$i.ProductState($code);$case.payload_after_failure=Test-Path $payload
  RegistrySnapshot $code ($kind+'-after-failure')
  $log=Get-Content (Join-Path $out "failure-$kind.log") -Raw;$op=if($kind-eq'install'){'Executing op: FileCopy'}else{'Executing op: FileRemove'};$before=$log.IndexOf($op);$after=$log.IndexOf('Doing action: DeliberateFailure');Require ($before-ge0-and$after-gt$before) 'Fault preceded actual file execution';$case.actual_file_operation_before_failure=$true;$case.skipped_rollback_errors=([regex]::Matches($log,'Error in rollback skipped')).Count
  $case.rollback_correct=if($kind-eq'install'){$case.state_after_failure-eq-1-and-not$case.payload_after_failure}else{$case.state_after_failure-eq5-and$case.payload_after_failure};Save
  $repair=if($i.ProductState($code)-eq-1){''}else{' REINSTALL=ALL REINSTALLMODE=amus'};$case.recovery_install_exit=MSI ('/i "'+$path+'"'+$repair) ("recovery-$kind");$case.state_after_recovery=$i.ProductState($code);$case.cleanup_exit=MSI ("/x $code") ("cleanup-$kind");$case.state_after_cleanup=$i.ProductState($code);$case.payload_after_cleanup=Test-Path $payload;Save
 }
 $null=[Runtime.InteropServices.Marshal]::FinalReleaseComObject($i);$i=$null
 $bad=@($result.cases|Where-Object {-not$_.rollback_correct-or$_.skipped_rollback_errors-ne0-or$_.recovery_install_exit-ne0-or$_.cleanup_exit-ne0-or$_.state_after_cleanup-ne-1-or$_.payload_after_cleanup})
 $result.status=if($bad.Count){'failed native rollback or supported cleanup'}else{'pass'}
}catch{$result.status='failed control execution';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally{if($i){$null=[Runtime.InteropServices.Marshal]::FinalReleaseComObject($i)};Save}
if($result.status-ne'pass'){exit 1}
