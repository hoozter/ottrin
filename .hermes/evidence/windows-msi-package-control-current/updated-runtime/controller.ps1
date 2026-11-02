param([Parameter(Mandatory=$true)][string]$Output)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$id=[Security.Principal.WindowsIdentity]::GetCurrent();if(-not([Security.Principal.WindowsPrincipal]::new($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Hosted controller requires its existing administrator token'}
$out=[IO.Path]::GetFullPath($Output);if(Test-Path $out){throw 'Refuse existing evidence root'};New-Item -ItemType Directory $out|Out-Null
$nonce=[Guid]::NewGuid().ToString('N');$root=Join-Path $env:TEMP ('ottrin-msi-'+$nonce);$name='OTMSI'+$nonce.Substring(0,12);$user=$null;$credential=$null;$password=$null;$process=$null;$allocated=$false;$exit=1
$state=@{scope='Fresh hosted Windows native file-only per-user controls; standalone QA identity,not an application publisher';nonce=$nonce;root=$root;account=$name;status='starting';profiles_before=@(Get-CimInstance Win32_UserProfile|ForEach-Object {$_.SID+'|'+$_.LocalPath}|Sort-Object)}
function Save {$state|ConvertTo-Json -Depth 8|Set-Content -Encoding UTF8 (Join-Path $out 'OWNER.json')}
function Grant([string]$Path,[Security.Principal.SecurityIdentifier]$Sid,[string]$Rights){$acl=Get-Acl $Path;$acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($Sid,$Rights,'ContainerInherit,ObjectInherit','None','Allow'));Set-Acl $Path $acl}
function VerifyDownload([string]$Url,[string]$Path,[string]$Algorithm,[string]$Digest){Invoke-WebRequest -UseBasicParsing $Url -OutFile $Path;if((Get-FileHash $Path -Algorithm $Algorithm).Hash-ne$Digest){throw 'Pinned compiler download mismatch'}}
try {
 Save;if(Test-Path $root){throw 'Foreign fixture retained'};New-Item -ItemType Directory $root|Out-Null;$allocated=$true
 $acl=[Security.AccessControl.DirectorySecurity]::new();$acl.SetAccessRuleProtection($true,$false);foreach($sid in @($id.User.Value,'S-1-5-18','S-1-5-32-544')|Select-Object -Unique){$acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new($sid),'FullControl','ContainerInherit,ObjectInherit','None','Allow'))};Set-Acl $root $acl
 $password=ConvertTo-SecureString ([Guid]::NewGuid().ToString('N')+'aA1!') -AsPlainText -Force
 $user=New-LocalUser -Name $name -Password $password -Description ('Ottrin MSI QA '+$nonce) -PasswordNeverExpires
 $state.sid=$user.SID.Value;Save;Add-LocalGroupMember -Group (Get-LocalGroup -SID 'S-1-5-32-545') -Member $user
 Grant $root $user.SID 'ReadAndExecute';New-Item -ItemType Directory (Join-Path $root 'results')|Out-Null;Grant (Join-Path $root 'results') $user.SID 'FullControl'
 foreach($file in @('application.msi','application-input.json','payload.json','config-template.json')){Copy-Item (Join-Path $PSScriptRoot $file) (Join-Path $root $file)}
 $packageInput=Get-Content (Join-Path $root 'application-input.json') -Raw|ConvertFrom-Json;if((Get-FileHash (Join-Path $root 'application.msi')).Hash.ToLowerInvariant()-ne$packageInput.sha256){throw 'Actual MSI input mismatch'}
 Copy-Item (Join-Path $PSScriptRoot 'actor.ps1') (Join-Path $root 'actor.ps1')
 $errors=$null;$tokens=$null;$null=[Management.Automation.Language.Parser]::ParseFile((Join-Path $root 'actor.ps1'),[ref]$tokens,[ref]$errors);if($errors.Count){throw ($errors|Out-String)}
 $credential=[Management.Automation.PSCredential]::new(($env:COMPUTERNAME+'\'+$name),$password);$state.phase='native actor';Save
 $invoke="& ([ScriptBlock]::Create([IO.File]::ReadAllText('$(Join-Path $root 'actor.ps1')'))) -Root '$root' -ExpectedSid '$($user.SID.Value)'"
 $arguments='-NoProfile -NonInteractive -WindowStyle Hidden -EncodedCommand '+[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($invoke))
 $action=New-ScheduledTaskAction -Execute "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe" -Argument $arguments
  $trigger=New-ScheduledTaskTrigger -AtLogOn -User ($env:COMPUTERNAME+'\'+$name)
  $principal=New-ScheduledTaskPrincipal -UserId ($env:COMPUTERNAME+'\'+$name) -LogonType Interactive -RunLevel Limited
  $taskName='OttrinOwnedMsiControl';Register-ScheduledTask -TaskName $taskName -Action $action -Trigger $trigger -Principal $principal -Force|Out-Null
  $plain=$credential.GetNetworkCredential().Password
  $key='HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon'
  foreach($pair in @(@('AutoAdminLogon','1'),@('DefaultUserName',$name),@('DefaultDomainName',$env:COMPUTERNAME),@('DefaultPassword',$plain))){Set-ItemProperty -Path $key -Name $pair[0] -Value $pair[1]}
  $plain=$null;$state.phase='ready for owned interactive reboot';$state.status='prepared';$state.task=$taskName;$state.actor_sid=$user.SID.Value;$exit=0;Save
 }catch{$state.status='failed interactive preparation';$state.error=$_.Exception.Message;$state.stack=$_.ScriptStackTrace;$exit=1}
 finally{if($password){$password.Dispose()};Save}
 exit $exit
 