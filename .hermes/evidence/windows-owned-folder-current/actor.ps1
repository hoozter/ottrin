$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$root='C:\OttrinOwnedFolderQA'
$result=@{status='running';checks=@()}
$p=$null
try {
 $identity=[Security.Principal.WindowsIdentity]::GetCurrent()
 $admin=([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
 $result.user=$identity.User.Value
 $result.owner=$identity.Owner.Value
 $result.administrator=$admin
 $phase=(Get-Content "$root\PHASE.txt" -Raw).Trim()
 $owner=Get-Content "$root\OWNER.json" -Raw|ConvertFrom-Json
 if($identity.User.Value-ne$owner.sid){throw 'Wrong owned test SID'}
 $tokenMode=$phase.Split('-')[0]
 if($tokenMode-eq'elevated'){
  if(-not$admin-or$identity.Owner-eq$identity.User){throw 'Fixture did not reproduce an elevated distinct default owner'}
 }elseif($tokenMode-eq'limited'){
  if($admin-or$identity.Owner-ne$identity.User){throw 'Fixture is not the expected ordinary token'}
 }else{throw 'Unknown owned phase'}
 $result.phase=$phase
 $homeRoot=Join-Path $root $phase
 if(Test-Path $homeRoot){throw 'Phase state already exists'}
 New-Item -ItemType Directory $homeRoot|Out-Null
 foreach($pair in @(@('TEMP','t'),@('TMP','t'),@('APPDATA','a'),@('LOCALAPPDATA','l'),@('USERPROFILE','h'),@('HOME','h'),@('XDG_CONFIG_HOME','c'),@('XDG_STATE_HOME','s'),@('XDG_DATA_HOME','d'),@('XDG_CACHE_HOME','cache'))){
  $path=Join-Path $homeRoot $pair[1]
  if(-not(Test-Path $path)){New-Item -ItemType Directory $path|Out-Null}
  [Environment]::SetEnvironmentVariable($pair[0],$path,'Process')
 }
 $filters=@('windows_native_move_private_directory_admits_its_owned_policy_and_refuses_replaced_parent','windows_native_move_cleanup','windows_native_replace_undo_admits_current_parents_without_rebinding_foreign_names')
 foreach($filter in $filters){
  $stdout="$root\results\$phase-$filter.stdout"
  $stderr="$root\results\$phase-$filter.stderr"
  $p=Start-Process "$root\platform.exe" -ArgumentList @($filter,'--nocapture','--test-threads=1') -WorkingDirectory $homeRoot -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
  $null=$p.Handle
  if(-not$p.WaitForExit(120000)){throw "Owned test remains live: $($p.Id)"}
  $exit=$p.ExitCode
  $text=Get-Content $stdout -Raw
  $result.checks+=@{filter=$filter;exit=$exit;stdout=[IO.Path]::GetFileName($stdout);stderr=[IO.Path]::GetFileName($stderr)}
  if($exit-ne0){throw "Native test failed: $filter, exit $exit"}
  if($text-notmatch'test result: ok\. [1-9][0-9]* passed; 0 failed;'){throw "Native filter did not execute passing tests: $filter"}
 }
 $result.temp_entries=@(Get-ChildItem -LiteralPath $env:TEMP -Force).Count
 if($result.temp_entries-ne0){throw 'Native fixtures left temporary files'}
 $result.status='pass'
}catch{
 $result.status='fail'
 $result.error=$_.Exception.Message
}finally{
 if($p-and-not$p.HasExited){& C:\Windows\System32\taskkill.exe /PID $p.Id /T /F|Out-Null;$null=$p.WaitForExit(10000)}
 $result|ConvertTo-Json -Depth 8|Set-Content "$root\results\$phase-RESULT.json"
}
if($result.status-ne'pass'){exit 1}
