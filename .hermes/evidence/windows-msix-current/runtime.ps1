param([Parameter(Mandatory=$true)][ValidateSet('Prepare','Install','Actor')][string]$Mode,[string]$Disc,[string]$Root='C:\OttrinOwnedMsixRuntime')
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
function Save($Name,$Value){$Value|ConvertTo-Json -Depth 10|Set-Content -Encoding UTF8 (Join-Path $Root ('results\'+$Name+'.json'))}
if($Mode-eq'Prepare'){
 if(Test-Path $Root){throw 'Existing runtime fixture retained'}
 $packageInput=Get-Content (Join-Path $Disc 'runtime-input.json') -Raw|ConvertFrom-Json
 if((Get-FileHash (Join-Path $Disc 'Ottrin.msix')).Hash.ToLowerInvariant()-ne$packageInput.sha256){throw 'Runtime package input mismatch'}
 New-Item -ItemType Directory $Root,(Join-Path $Root 'results')|Out-Null
 $nonce=[Guid]::NewGuid().ToString('N');$name='OTMSIX'+$nonce.Substring(0,12);$secret=[Guid]::NewGuid().ToString('N')+'aA1!';$password=ConvertTo-SecureString $secret -AsPlainText -Force
 try {
  $user=New-LocalUser -Name $name -Password $password -Description ('Owned MSIX QA '+$nonce) -PasswordNeverExpires
  foreach($group in @('S-1-5-32-545','S-1-5-32-544')){Add-LocalGroupMember -Group (Get-LocalGroup -SID $group) -Member $user}
  $acl=Get-Acl $Root;$acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($user.SID,'ReadAndExecute','ContainerInherit,ObjectInherit','None','Allow'));Set-Acl $Root $acl
  $acl=Get-Acl (Join-Path $Root 'results');$acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($user.SID,'Modify','ContainerInherit,ObjectInherit','None','Allow'));Set-Acl (Join-Path $Root 'results') $acl
  Copy-Item (Join-Path $Disc 'Ottrin.msix'),(Join-Path $Disc 'runtime.ps1'),(Join-Path $Disc 'native-controls.ps1') $Root
  $owner=@{nonce=$nonce;account=$name;sid=$user.SID.Value;package=$packageInput;installer_task=('OttrinMsixInstall-'+$nonce);actor_task=('OttrinMsixActor-'+$nonce);scope='Disposable clone only; administrative unsigned deployment, then a filtered ordinary runtime token; no signing-policy changes'}
  $owner|ConvertTo-Json -Depth 6|Set-Content -Encoding UTF8 (Join-Path $Root 'OWNER.json')
  foreach($phase in @('Install','Actor')){
   $invoke="& ([ScriptBlock]::Create([IO.File]::ReadAllText('$Root\runtime.ps1'))) -Mode $phase -Root '$Root'"
   $action=New-ScheduledTaskAction -Execute "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe" -Argument ('-NoProfile -NonInteractive -WindowStyle Hidden -EncodedCommand '+[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($invoke)))
   $principal=New-ScheduledTaskPrincipal -UserId ($env:COMPUTERNAME+'\'+$name) -LogonType Interactive -RunLevel $(if($phase-eq'Install'){'Highest'}else{'Limited'})
   if($phase-eq'Install'){$trigger=New-ScheduledTaskTrigger -AtLogOn -User ($env:COMPUTERNAME+'\'+$name);Register-ScheduledTask -TaskName $owner.installer_task -Action $action -Principal $principal -Trigger $trigger|Out-Null}else{Register-ScheduledTask -TaskName $owner.actor_task -Action $action -Principal $principal|Out-Null}
  }
  $key='HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon'
  foreach($pair in @(@('AutoAdminLogon','1'),@('DefaultUserName',$name),@('DefaultDomainName',$env:COMPUTERNAME),@('DefaultPassword',$secret))){Set-ItemProperty $key -Name $pair[0] -Value $pair[1]}
  @{status='prepared';account=$name;sid=$user.SID.Value;scope=$owner.scope}|ConvertTo-Json -Compress
 }finally{$secret=$null;$password.Dispose()}
 exit 0
}
$owner=Get-Content (Join-Path $Root 'OWNER.json') -Raw|ConvertFrom-Json
$identity=[Security.Principal.WindowsIdentity]::GetCurrent();if($identity.User.Value-ne$owner.sid-or(Get-Process -Id $PID).SessionId-le0){throw 'Wrong owned interactive identity'}
$admin=([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if($Mode-eq'Install'){
 $result=@{status='running';scope=$owner.scope;source_revision=$owner.package.source_revision;actor_sid=$owner.sid}
 try {
  if(-not$admin){throw 'Unsigned development deployment requires the explicit administrative QA token'}
  if(@(Get-AppxPackage -Name 'Ottrin.Development').Count){throw 'Unexpected existing development package'}
  Add-AppxPackage -Path (Join-Path $Root 'Ottrin.msix') -AllowUnsigned
  $package=Get-AppxPackage -Name 'Ottrin.Development';if(-not$package){throw 'Package registration missing'}
  $result.deployment=@{full_name=$package.PackageFullName;family=$package.PackageFamilyName;location=$package.InstallLocation;version=[string]$package.Version};Save 'DEPLOYMENT' $result
  Start-ScheduledTask -TaskName $owner.actor_task;$deadline=[DateTime]::UtcNow.AddMinutes(6)
  while([DateTime]::UtcNow-lt$deadline){$path=Join-Path $Root 'results\ACTOR.json';if(Test-Path $path){$actor=Get-Content $path -Raw|ConvertFrom-Json;if($actor.status-ne'running'){break}};Start-Sleep -Seconds 1}
  if(-not$actor-or$actor.status-eq'running'){throw 'Ordinary runtime actor timed out'}
  $result.actor=$actor
  Remove-AppxPackage -Package $package.PackageFullName
  $result.package_removed=(@(Get-AppxPackage -Name 'Ottrin.Development').Count-eq0)
  $tasks=@(Get-ScheduledTask|Where-Object {
   if($_.TaskName-notlike'Ottrin Index*'){return $false}
   $principal=[string]$_.Principal.UserId
   $sid=if($principal-like'S-1-*'){$principal}else{([Security.Principal.NTAccount]::new($principal)).Translate([Security.Principal.SecurityIdentifier]).Value}
   $sid-eq$owner.sid
  })
  $result.remaining_index_tasks=@($tasks|ForEach-Object {@{name=$_.TaskName;enabled=$_.Settings.Enabled;actions=@($_.Actions|ForEach-Object {$_.Execute})}})
  $result.fixture_preserved=(Get-Content (Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'OttrinMSIXFixture\preserve.txt') -Raw)-eq"Preserve owned MSIX fixture`n"
  $result.status=if($actor.status-eq'pass'-and$result.package_removed-and$tasks.Count-eq0-and$result.fixture_preserved){'pass'}else{'failed runtime or package-removal gate'}
 }catch{$result.status='failed native MSIX lifecycle';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
 finally{Save 'RESULT' $result}
 exit $(if($result.status-eq'pass'){0}else{1})
}
$result=@{status='running';administrator=$admin;session_id=(Get-Process -Id $PID).SessionId;scope='Actual packaged ordinary main-window launch, normal close and optional index registration; no upgrade acceptance yet'};Save 'ACTOR' $result
try {
 if($admin){throw 'Ordinary application token required'}
 $fixture=Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'OttrinMSIXFixture';New-Item -ItemType Directory $fixture|Out-Null;[IO.File]::WriteAllText((Join-Path $fixture 'preserve.txt'),"Preserve owned MSIX fixture`n")
 $config=Join-Path $env:APPDATA 'ottrin\config.json';New-Item -ItemType Directory (Split-Path $config) -Force|Out-Null
 [IO.File]::WriteAllText($config,(@{theme='Light';default_view_mode='Miller';bookmarks=@();installed_office_previews_enabled=$false;search=@{include_roots=@($fixture);content_indexing_enabled=$false}}|ConvertTo-Json -Depth 5),[Text.UTF8Encoding]::new($false))
 $package=Get-AppxPackage -Name 'Ottrin.Development';$exe=Join-Path $package.InstallLocation 'bin\ottrin.exe'
 Add-Type -TypeDefinition @'
using System;using System.Text;using System.Runtime.InteropServices;
public static class InputDesktopQA {
 [DllImport("user32.dll",SetLastError=true)]static extern IntPtr OpenInputDesktop(uint flags,bool inherit,uint access);
 [DllImport("user32.dll",CharSet=CharSet.Unicode,SetLastError=true)]static extern bool GetUserObjectInformation(IntPtr value,int index,StringBuilder text,uint length,out uint needed);
 [DllImport("user32.dll")]static extern bool CloseDesktop(IntPtr value);
 public static string Name(){var h=OpenInputDesktop(0,false,1);if(h==IntPtr.Zero)return "unavailable";try{var b=new StringBuilder(256);uint needed;return GetUserObjectInformation(h,2,b,512,out needed)?b.ToString():"unavailable";}finally{CloseDesktop(h);}}
}
'@
 $readyDeadline=[DateTime]::UtcNow.AddMinutes(2)
 while([InputDesktopQA]::Name()-ne'Default'-and[DateTime]::UtcNow-lt$readyDeadline){Start-Sleep -Seconds 1}
 $result.input_desktop=[InputDesktopQA]::Name();Save 'ACTOR' $result
 & ([ScriptBlock]::Create([IO.File]::ReadAllText((Join-Path $Root 'native-controls.ps1'))));[PickerNative]::Init()
 Start-Process explorer.exe -ArgumentList ('shell:AppsFolder\'+$package.PackageFamilyName+'!Ottrin')|Out-Null
 $deadline=[DateTime]::UtcNow.AddSeconds(60);$app=$null;$window=[IntPtr]::Zero
 while([DateTime]::UtcNow-lt$deadline){
  foreach($candidate in @(Get-Process ottrin -ErrorAction SilentlyContinue|Where-Object {$_.Path-ieq$exe})){
   $found=[PickerNative]::Window($candidate.Id,'Ottrin')
   if($found-ne[IntPtr]::Zero){$app=$candidate;$window=$found;break}
  }
  if($app){break};Start-Sleep -Milliseconds 200
 }
 if(-not$app){throw 'Actual packaged main window did not map'}
 $processOwner=Invoke-CimMethod -InputObject (Get-CimInstance Win32_Process -Filter ("ProcessId="+$app.Id)) -MethodName GetOwnerSid
 if($processOwner.Sid-ne$owner.sid){throw 'Main window belongs to another identity'}
 Add-Type -TypeDefinition 'using System; using System.Text; using System.Runtime.InteropServices; public static class PackageIdentityQA { [DllImport("kernel32.dll", CharSet=CharSet.Unicode)] public static extern int GetPackageFullName(IntPtr process, ref uint length, StringBuilder value); [DllImport("user32.dll", SetLastError=true)] public static extern bool PostMessageW(IntPtr window, uint message, IntPtr first, IntPtr second); }'
 [uint32]$length=0;$errorCode=[PackageIdentityQA]::GetPackageFullName($app.Handle,[ref]$length,$null)
 if($errorCode-ne122){throw 'Main process has no queryable package identity'}
 $buffer=[Text.StringBuilder]::new([int]$length)
 if([PackageIdentityQA]::GetPackageFullName($app.Handle,[ref]$length,$buffer)-ne0-or$buffer.ToString()-ne$package.PackageFullName){throw 'Main process package identity mismatch'}
 $result.actual_process_package=$buffer.ToString()
 [PickerNative]::Place($window)
 try {
  if($result.input_desktop-ne'Default'){throw 'Fresh-login input desktop not ready'}
  [PickerNative]::Focus($window);Start-Sleep -Milliseconds 500
  [PickerNative]::Shot($window,(Join-Path $Root 'results\main.png'));$result.gui_focus_and_capture='pass'
 }catch{$result.gui_focus_and_capture='failed';$result.gui_error=$_.Exception.Message}
 $result.main=@{pid=$app.Id;path=$app.Path;title=[PickerNative]::Title($window);binary_sha256=(Get-FileHash $app.Path).Hash};Save 'ACTOR' $result
 try {
  if([PickerNative]::Window($app.Id,'Ottrin')-ne$window){throw 'Owned main window identity changed'}
  if(-not[PackageIdentityQA]::PostMessageW($window,0x10,[IntPtr]::Zero,[IntPtr]::Zero)){throw 'Normal window close was refused'}
  if(-not$app.WaitForExit(15000)){throw 'Main window did not close'};$app.WaitForExit();if($app.ExitCode-ne0){throw 'Main window exited abnormally'};$result.normal_close_exit=0
 }catch{$result.normal_close_error=$_.Exception.Message}
 $process=Start-Process $exe -ArgumentList @('index','service-install') -PassThru -Wait -WindowStyle Hidden -RedirectStandardOutput (Join-Path $Root 'results\service-install.stdout') -RedirectStandardError (Join-Path $Root 'results\service-install.stderr')
 $result.service_install_exit=$process.ExitCode;if($process.ExitCode-ne0){throw 'Packaged background index setup failed; inspect exact CLI output'}
 $result.status=if($result.gui_focus_and_capture-eq'pass'-and$result.ContainsKey('normal_close_exit')-and$result.normal_close_exit-eq0){'pass'}else{'partial runtime; GUI focus or close gate failed'}
}catch{$result.status='failed ordinary runtime';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally{Save 'ACTOR' $result}
exit $(if($result.status-eq'pass'){0}else{1})
