param([Parameter(Mandatory=$true)][string]$Output)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
if($env:GITHUB_ACTIONS-ne'true'-or$env:RUNNER_OS-ne'Windows'-or$env:RUNNER_ENVIRONMENT-ne'github-hosted'){throw 'Disposable hosted Windows runner required'}
$id=[Security.Principal.WindowsIdentity]::GetCurrent();if(-not([Security.Principal.WindowsPrincipal]::new($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Hosted controller requires its existing administrator token'}
$out=[IO.Path]::GetFullPath($Output);if(Test-Path $out){throw 'Refuse existing evidence root'};New-Item -ItemType Directory $out|Out-Null
$nonce=[Guid]::NewGuid().ToString('N');$root=Join-Path $env:RUNNER_TEMP ('ottrin-msi-'+$nonce);$name='OTMSI'+$nonce.Substring(0,12);$user=$null;$credential=$null;$password=$null;$process=$null;$allocated=$false;$exit=1
$state=@{scope='Fresh hosted Windows native file-only per-user controls; standalone QA identity,not an application publisher';nonce=$nonce;root=$root;account=$name;status='starting';profiles_before=@(Get-CimInstance Win32_UserProfile|ForEach-Object {$_.SID+'|'+$_.LocalPath}|Sort-Object)}
function Save {$state|ConvertTo-Json -Depth 8|Set-Content -Encoding UTF8 (Join-Path $out 'OWNER.json')}
function Grant([string]$Path,[Security.Principal.SecurityIdentifier]$Sid,[string]$Rights){$acl=Get-Acl $Path;$acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($Sid,$Rights,'ContainerInherit,ObjectInherit','None','Allow'));Set-Acl $Path $acl}
function VerifyDownload([string]$Url,[string]$Path,[string]$Algorithm,[string]$Digest){Invoke-WebRequest -UseBasicParsing $Url -OutFile $Path;if((Get-FileHash $Path -Algorithm $Algorithm).Hash-ne$Digest){throw 'Pinned compiler download mismatch'}}
try {
 Save;if(Test-Path $root){throw 'Foreign fixture retained'};New-Item -ItemType Directory $root|Out-Null;$allocated=$true
 $acl=[Security.AccessControl.DirectorySecurity]::new();$acl.SetAccessRuleProtection($true,$false);foreach($sid in @($id.User.Value,'S-1-5-18','S-1-5-32-544')|Select-Object -Unique){$acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new($sid),'FullControl','ContainerInherit,ObjectInherit','None','Allow'))};Set-Acl $root $acl
 $password=ConvertTo-SecureString ([Guid]::NewGuid().ToString('N')+'aA1!') -AsPlainText -Force
 $user=New-LocalUser -Name $name -Password $password -Description ('Ottrin owned hosted MSI QA '+$nonce) -PasswordNeverExpires
 $state.sid=$user.SID.Value;Save;Add-LocalGroupMember -Group (Get-LocalGroup -SID 'S-1-5-32-545') -Member $user
 Grant $root $user.SID 'ReadAndExecute';New-Item -ItemType Directory (Join-Path $root 'results')|Out-Null;Grant (Join-Path $root 'results') $user.SID 'FullControl'
 $repo=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'));$pins=Get-Content (Join-Path $repo '.hermes\evidence\windows-20261011\windows-msi-compiler-downloads.json') -Raw|ConvertFrom-Json
 VerifyDownload $pins.runtime_url (Join-Path $root 'runtime.zip') 'SHA512' $pins.runtime_sha512;VerifyDownload $pins.wix_url (Join-Path $root 'wix.zip') 'SHA256' $pins.wix_sha256
 Expand-Archive (Join-Path $root 'runtime.zip') (Join-Path $root 'dotnet');Expand-Archive (Join-Path $root 'wix.zip') (Join-Path $root 'wix')
 $template=@'
<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs">
 <Package Name="Ottrin Hosted Rollback Control KIND" Manufacturer="Ottrin QA" Version="1.0.0" UpgradeCode="UPGRADE" Scope="perUser" Language="1033">
  <MediaTemplate EmbedCab="yes" />
  <StandardDirectory Id="LocalAppDataFolder"><Directory Id="CONTROLFOLDER" Name="OttrinHostedRollback-KIND">
   <Component Id="ControlComponent" Guid="COMPONENT">
    <File Id="ControlFile" Source="$(var.Payload)" />
    <RegistryValue Root="HKCU" Key="Software\OttrinQA\HostedRollback\NONCE\KIND" Name="Owned" Type="integer" Value="1" KeyPath="yes" />
    <RemoveFolder Id="ControlFolderCleanup" On="uninstall" />
   </Component>
  </Directory></StandardDirectory>
  <Feature Id="Main" Level="1"><ComponentRef Id="ControlComponent" /></Feature>
  <CustomAction Id="DeliberateFailure" Error="Owned rollback control failed after execution." />
  <InstallExecuteSequence>
   <InstallExecute Sequence="6500" Condition="1" />
   <Custom Action="DeliberateFailure" After="InstallExecute" Condition="OTTRIN_QA_FAIL=1" />
  </InstallExecuteSequence>
 </Package>
</Wix>
'@
 [IO.File]::WriteAllText((Join-Path $root 'control.txt'),'Ottrin hosted rollback control',[Text.UTF8Encoding]::new($false))
 foreach($kind in @('install','uninstall')){
  $source=$template.Replace('KIND',$kind).Replace('UPGRADE',[Guid]::NewGuid().ToString()).Replace('COMPONENT',[Guid]::NewGuid().ToString()).Replace('NONCE',$nonce);$wxs=Join-Path $root "control-$kind.wxs";[IO.File]::WriteAllText($wxs,$source,[Text.UTF8Encoding]::new($false))
  & (Join-Path $root 'dotnet\dotnet.exe') (Join-Path $root 'wix\tools\net6.0\any\wix.dll') build -arch x64 -d ('Payload='+(Join-Path $root 'control.txt')) -o (Join-Path $root "control-$kind.msi") $wxs
  if($LASTEXITCODE-ne0){throw 'Native compiler failed'};Copy-Item $wxs $out
 }
 Copy-Item (Join-Path $PSScriptRoot 'msi-rollback-control-actor.ps1') (Join-Path $root 'actor.ps1')
 $errors=$null;$tokens=$null;$null=[Management.Automation.Language.Parser]::ParseFile((Join-Path $root 'actor.ps1'),[ref]$tokens,[ref]$errors);if($errors.Count){throw ($errors|Out-String)}
 $credential=[Management.Automation.PSCredential]::new(($env:COMPUTERNAME+'\'+$name),$password);$state.phase='native actor';Save
 $arguments='-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "'+(Join-Path $root 'actor.ps1')+'" -Root "'+$root+'" -ExpectedSid "'+$user.SID.Value+'"'
 $process=Start-Process "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe" -Credential $credential -LoadUserProfile -ArgumentList $arguments -PassThru;$null=$process.Handle
 if(-not$process.WaitForExit(360000)){$process.Kill();$null=$process.WaitForExit(10000);throw 'Owned native actor timed out'};$process.WaitForExit();$exit=$process.ExitCode;$state.actor_exit=$exit
 Copy-Item (Join-Path $root 'results\*') $out;$state.status=if($exit-eq0){'pass'}else{'native control failed; see RESULT.json'}
}catch{$state.status='failed controller';$state.error=$_.Exception.Message;$state.stack=$_.ScriptStackTrace;$exit=1}
finally {
 try {
  if($process){if(-not$process.HasExited){$process.Kill();$null=$process.WaitForExit(10000)};$process.Dispose()}
  $track=Join-Path $root 'results\active-msi.json'
  if(Test-Path $track){
   $owned=Get-Content $track -Raw|ConvertFrom-Json;$child=Get-Process -Id $owned.pid -ErrorAction SilentlyContinue
   if($child){$cim=Get-CimInstance Win32_Process -Filter ("ProcessId="+$owned.pid);$owner=Invoke-CimMethod -InputObject $cim -MethodName GetOwnerSid;if($owner.Sid-ne$user.SID.Value-or$child.StartTime.ToUniversalTime().Ticks-ne$owned.started-or$child.MainModule.FileName-ine"$env:SystemRoot\System32\msiexec.exe"-or-not$cim.CommandLine.Contains($root)){throw 'Owned MSI identity changed'};$child.Kill();if(-not$child.WaitForExit(10000)){throw 'Owned MSI process remains'};$child.Dispose()};Remove-Item $track
  }
  if($allocated-and(Test-Path (Join-Path $root 'results'))){Copy-Item (Join-Path $root 'results\*') $out -ErrorAction Stop}
  if($user){$current=Get-LocalUser -SID $user.SID;if($current.Description-ne('Ottrin owned hosted MSI QA '+$nonce)){throw 'Account ownership changed'};$ownedProfile=Get-CimInstance Win32_UserProfile|Where-Object {$_.SID-eq$user.SID.Value};if($ownedProfile){$deadline=[DateTime]::UtcNow.AddSeconds(30);while($ownedProfile.Loaded-and[DateTime]::UtcNow-lt$deadline){Start-Sleep -Milliseconds 500;$ownedProfile=Get-CimInstance Win32_UserProfile|Where-Object {$_.SID-eq$user.SID.Value}};if($ownedProfile.Loaded){throw 'Owned profile remains loaded'};if($ownedProfile){$ownedProfile|Remove-CimInstance}};Remove-LocalUser -SID $user.SID;if(Get-LocalUser -Name $name -ErrorAction SilentlyContinue){throw 'Owned account remains'}}
  if($allocated){Remove-Item $root -Recurse -Force;if(Test-Path $root){throw 'Owned fixture remains'}}
  $after=@(Get-CimInstance Win32_UserProfile|ForEach-Object {$_.SID+'|'+$_.LocalPath}|Sort-Object);if(Compare-Object $state.profiles_before $after){throw 'Original profiles changed'}
  $state.cleanup=@{owned_root_removed=$true;owned_account_profile_removed=$true;original_profiles_preserved=$true;hosted_runner_disposal_handles_any_residual_engine_registration=$true}
 }catch{$state.cleanup_error=$_.Exception.Message;$state.status='failed teardown';$exit=1}
 if($password){$password.Dispose()};Save
}
exit $exit
