$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$b='C:\OttrinTreeUIQA';$r="$b\results";$install="$env:LOCALAPPDATA\Programs\Ottrin";$app="$install\bin\ottrin.exe";$active=@();$installed=$false
function Assert($v,$m){if(-not$v){throw $m}}
function Run($exe,$arguments){$p=Start-Process $exe -ArgumentList $arguments -PassThru -RedirectStandardOutput "$r\index-control.out" -RedirectStandardError "$r\index-control.err";$null=$p.Handle;$script:active+=,$p;Assert ($p.WaitForExit(120000)) 'Control process remains live';Assert ($p.ExitCode-eq0) "Control command failed $exe ($($p.ExitCode))"}
. "$b\snapshot.ps1"
$source="$env:USERPROFILE\TreeQA2\same-directory-unoccupied\s\bundle "+[char]0x3A9;$target="$env:USERPROFILE\TreeQA2\same-directory-unoccupied\d\bundle "+[char]0x3A9
$result=@{status='running';scope='Ordinary-user same directory move with configured indexer running, then stopped; no GUI'}
try{
 Assert ($env:USERNAME-eq'OttrinTreeUIQA') 'Wrong actor';Assert (-not(Test-Path $install)) 'Product already installed';$installed=$true;Run "$b\current.exe" '/S';Run $app 'index update --json';$before=@(Snapshot $source);$result.before=$before
 $daemon=@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue|Where-Object Path -eq "$install\bin\ottrin-indexd.exe");Assert ($daemon.Count-eq1) 'Expected one own daemon';$result.daemon_pid=$daemon[0].Id
 try{[IO.Directory]::Move($source,$target);$result.running_move='succeeded';Assert (EqualSnapshot @(Snapshot $target) $before $true) 'Running move changed tree';[IO.Directory]::Move($target,$source)}catch{$result.running_move='failed';$result.running_error=$_.Exception.Message;$result.running_hresult=$_.Exception.GetBaseException().HResult}
 Assert ((Test-Path -LiteralPath $source)-and-not(Test-Path -LiteralPath $target)) 'Unexpected post-running topology';Assert (EqualSnapshot @(Snapshot $source) $before $true) 'Running attempt changed source'
 Run "$install\bin\ottrin-indexd.exe" '--stop';Assert (@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue|Where-Object Path -eq "$install\bin\ottrin-indexd.exe").Count-eq0) 'Daemon remains'
 [IO.Directory]::Move($source,$target);Assert (EqualSnapshot @(Snapshot $target) $before $true) 'Stopped move changed tree';[IO.Directory]::Move($target,$source);Assert (EqualSnapshot @(Snapshot $source) $before $true) 'Restore changed tree';$result.stopped_move='succeeded-and-restored';$result.status='pass'
}catch{$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally{foreach($p in $active){if(-not$p.HasExited){& "$env:WINDIR\System32\taskkill.exe" /PID $p.Id /T /F|Out-Null}};try{if($installed-and(Test-Path "$install\Uninstall.exe")){Copy-Item "$install\Uninstall.exe" "$r\control-uninstall.exe" -Force;Run "$r\control-uninstall.exe" "/S _?=$install";Assert (-not(Test-Path $install)) 'Control cleanup left payload'}}catch{$result.status='fail';$result.cleanup_error=$_.Exception.Message};$result|ConvertTo-Json -Depth 20|Set-Content "$r\INDEX_CONTROL.json" -Encoding UTF8}
if($result.status-ne'pass'){exit 1}
