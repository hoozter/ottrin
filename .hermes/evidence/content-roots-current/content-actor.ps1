param([ValidateSet('Portable','NSIS','MSI')][string]$Mode='Portable')
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
$b='C:\OttrinContentQA';$r="$b\results";$work="$env:USERPROFILE\ContentQA";$install="$env:LOCALAPPDATA\Programs\Ottrin";$owned=@();$installed=$false;$code=$null;$result=@{status='running';mode=$Mode;scope='Public packaged CLI as ordinary user with clean PATH; bounded positive installer lifecycle only'}
function Assert($ok,$message){if(-not$ok){throw $message}}
function Process($exe,$arguments,$expected=0){
 $s=New-Object Diagnostics.ProcessStartInfo($exe,$arguments);$s.WorkingDirectory=$script:cwd;$s.UseShellExecute=$false;$s.RedirectStandardOutput=$true;$s.RedirectStandardError=$true;$s.StandardOutputEncoding=[Text.Encoding]::UTF8;$s.StandardErrorEncoding=[Text.Encoding]::UTF8;$s.CreateNoWindow=$true
 $p=[Diagnostics.Process]::Start($s);$script:owned+=,$p;$o=$p.StandardOutput.ReadToEndAsync();$e=$p.StandardError.ReadToEndAsync();Assert ($p.WaitForExit(120000)) "Owned process still live: $($p.Id)";Assert ($o.Wait(5000)-and$e.Wait(5000)) 'Output open';Assert ($p.ExitCode-eq$expected) "Unexpected exit $($p.ExitCode): $arguments $($o.Result) $($e.Result)";return @{stdout=$o.Result;stderr=$e.Result}
}
function Query($arguments,$expected=0){$q=Process $script:app $arguments $expected;return ($q.stdout|ConvertFrom-Json)}
function SaveConfig{[IO.File]::WriteAllText($script:pref,($script:cfg|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false))}
function Paths($answer){return @($answer.items|ForEach-Object {$v=$_.path;if($v.StartsWith('\\?\')){$v=$v.Substring(4)};[IO.Path]::GetFullPath($v)}|Sort-Object)}
function CheckPaths($answer,$expected){$got=@(Paths $answer);$want=@($expected|Sort-Object);Assert (($got-join'|')-ceq($want-join'|')) "Unexpected paths: $($got-join'|') versus $($want-join'|')"}
function WaitSaved($term,$expected){$until=[DateTime]::UtcNow.AddSeconds(15);do{$answer=Query "search `"$term !neverpresentterm`" --content --json";$got=@(Paths $answer);$want=@($expected|Sort-Object);if(($got-join'|')-ceq($want-join'|')){return};Start-Sleep -Milliseconds 75}while([DateTime]::UtcNow-lt$until);throw "Saved content watcher misses $term : $($got-join'|')"}
function WaitUninstalled { $until=[DateTime]::UtcNow.AddSeconds(60);while((Test-Path "$script:install\bin\ottrin.exe")-or(Test-Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Ottrin')){if([DateTime]::UtcNow-ge$until){throw 'Normal uninstall did not finish'};Start-Sleep -Milliseconds 200} }
function Msi($action,$label){$p=Start-Process msiexec.exe -ArgumentList "$action /qn /norestart /l*v $r\$Mode-$label.log" -PassThru;$null=$p.Handle;$script:owned+=,$p;Assert ($p.WaitForExit(180000)) "Owned MSI still live $($p.Id)";Assert ($p.ExitCode-eq0) "MSI $label failed $($p.ExitCode)"}
try {
 Assert ($env:USERNAME-eq'OttrinContentQA') 'Wrong user';$id=[Security.Principal.WindowsIdentity]::GetCurrent();Assert (-not([Security.Principal.WindowsPrincipal]::new($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Actor elevated'
 Assert (-not(Test-Path $work)) 'Owned fixture already exists';New-Item -ItemType Directory "$work\metadata","$work\bodies","$work\other"|Out-Null
 $cwd="$env:WINDIR\System32";$env:PATH="$env:WINDIR\System32;$env:WINDIR;$env:WINDIR\System32\WindowsPowerShell\v1.0";Assert (-not(Get-Command rg -ErrorAction SilentlyContinue)) 'System RG unexpectedly available'
 $env:RIPGREP_CONFIG_PATH="$work\rg-config";[IO.File]::WriteAllText($env:RIPGREP_CONFIG_PATH,'--glob=!*.txt')
 if($Mode-eq'Portable'){Copy-Item "$b\payload\Ottrin-0.1.0-windows-x64" "$work\portable" -Recurse;$payload="$work\portable"}
 else {
  Assert (-not(Test-Path $install)) 'Existing installation';$installed=$true
  if($Mode-eq'NSIS'){$null=Process "$b\setup.exe" '/S'}
  else {$i=New-Object -ComObject WindowsInstaller.Installer;$db=$i.OpenDatabase("$b\current.msi",0);$v=$db.OpenView("SELECT ``Value`` FROM ``Property`` WHERE ``Property``='ProductCode'");$null=$v.Execute();$code=$v.Fetch().StringData(1);$v.Close();Msi "/i $b\current.msi" 'install';Assert ($i.ProductState($code)-eq5) 'MSI registration absent'}
  $payload=$install
 }
 $manifest=Get-Content "$b\payload.json" -Raw|ConvertFrom-Json;foreach($entry in $manifest.PSObject.Properties){Assert ((Get-FileHash (Join-Path $payload $entry.Name)).Hash.ToLowerInvariant()-ceq$entry.Value) "Payload differs $($entry.Name)"};$result.payload_files=@($manifest.PSObject.Properties).Count
 $app="$payload\bin\ottrin.exe";$provider="$payload\bin\rg.exe"
 $marker='ottrinproviderneedle';$hit="$work\bodies\r$([char]0xe9)sum$([char]0xe9).txt";$namehit="$work\metadata\$marker.txt"
 [IO.File]::WriteAllText($hit,$marker);[IO.File]::WriteAllText($namehit,'filename match');[IO.File]::WriteAllText("$work\bodies\large.txt",$marker+('x'*100));[IO.File]::WriteAllText("$work\bodies\excluded.txt",$marker);[IO.File]::WriteAllText("$work\bodies\wrong.bin",$marker)
 $pref="$env:APPDATA\ottrin\config.json";New-Item -ItemType Directory -Force (Split-Path $pref)|Out-Null
 $cfg=@{theme='Dark';default_view_mode='Miller';list_columns=@();command_frame_always_visible=$false;restore_session=$false;show_hidden_files=$false;target=@{current=$null};last_session_dirs=@();search=@{include_roots=@("$work\metadata");exclude_roots=@();content_indexing_enabled=$true;content_include_roots=@("$work\bodies");content_exclude_roots=@("$work\bodies\excluded.txt");content_file_groups=@('TextDocuments');content_max_file_size_bytes=64;use_system_locate=$false;manage_locate_db=$false;refresh_interval_hours=0;default_scope='Global'}};SaveConfig
 $status=Query 'index update --json';Assert ($status.status-eq'ready'-and$status.configured_roots-eq2) 'Configured private index not ready'
 WaitSaved $marker @($hit)
 [IO.File]::WriteAllText($hit,'updatedneedle');WaitSaved 'updatedneedle' @($hit);WaitSaved $marker @()
 New-Item -ItemType Directory "$work\bodies\new\deep"|Out-Null
 [IO.File]::WriteAllText("$work\bodies\new\deep\document.txt",'watcherneedle');WaitSaved 'watcherneedle' @("$work\bodies\new\deep\document.txt")
 [IO.Directory]::Move("$work\bodies\new","$work\bodies\moved");WaitSaved 'watcherneedle' @("$work\bodies\moved\deep\document.txt")
 Remove-Item "$work\bodies\moved" -Recurse -Force;WaitSaved 'watcherneedle' @()
 [IO.File]::WriteAllText($hit,$marker);WaitSaved $marker @($hit)
 $cfg.search.content_indexing_enabled=$false;SaveConfig;$status=Query 'index update --json';Assert ($status.configured_roots-eq1-and$status.watched_roots-eq1) 'Content watch did not detach'
 $cfg.search.content_indexing_enabled=$true;$cfg.search.include_roots=@();SaveConfig;$status=Query 'index update --json';Assert ($status.configured_roots-eq1) 'Content-only root missing'
 WaitSaved $marker @($hit);CheckPaths (Query "search $marker --json") @()
 $cfg.search.include_roots=@("$work\metadata");SaveConfig;$null=Query 'index update --json';WaitSaved $marker @($hit)
 $null=Process "$payload\bin\ottrin-indexd.exe" '--stop'
 CheckPaths (Query "search $marker --json") @($namehit)
 $answer=Query "search $marker --content --json";CheckPaths $answer @($namehit,$hit);Assert ($null-eq$answer.error) 'Provider error'
 $cfg.search.default_scope='CurrentFolder';SaveConfig;$cwd="$work\bodies";CheckPaths (Query "search $marker --content --json") @($hit);$cwd="$env:WINDIR\System32";CheckPaths (Query "search $marker --content --json") @()
 $cfg.search.default_scope='Global';SaveConfig
 if($Mode-eq'Portable'){
  Move-Item $provider ($provider+'.held');$failed=Query "search $marker --content --json" 1;Assert ($failed.error-like'Live content search*') 'Missing provider error absent';CheckPaths $failed @($namehit,$hit);CheckPaths (Query "search $marker --json") @($namehit)
  $cfg.search.content_indexing_enabled=$false;SaveConfig;$disabled=Process $app "search $marker --content --json" 1;Assert ($disabled.stderr-like'*Settings*') 'Disabled policy error absent';Move-Item ($provider+'.held') $provider
 } else {
  $null=Process $app 'index service-install';$answer=Query "search $marker --content --json";CheckPaths $answer @($hit,$namehit);$result.scheduled_daemon_content_query='pass'
  if($Mode-eq'NSIS'){$null=Process "$install\Uninstall.exe" '/S';WaitUninstalled}else{Msi "/x $code" 'uninstall';Assert ($i.ProductState($code)-eq-1) 'MSI still registered'}
  Assert (-not(Test-Path "$install\bin\ottrin.exe")) 'Installed app remains';$installed=$false
 }
 $result.checks=@('All payload identities','Clean PATH and external config ignored','Unicode live-content hit outside metadata root','Size/type/exclusion policy','Current-folder scope','Filename results preserved','Saved-only queries verify extraction and watcher edits/moves/deletion','Disabled content detaches its watches','Zero metadata roots retain content without filename membership');if($Mode-eq'Portable'){$result.checks+=@('Missing-provider error','Disabled content error')};$result.status='pass'
} catch {$result.status='fail';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally {
 $result|ConvertTo-Json -Depth 10|Set-Content "$r\CONTENT_$Mode.json" -Encoding UTF8
 if($payload-and(Test-Path "$payload\bin\ottrin-indexd.exe")){try{$live=@(Get-Process ottrin-indexd -ErrorAction SilentlyContinue|Where-Object {$_.Path-eq"$payload\bin\ottrin-indexd.exe"});if($live.Count){$null=Process "$payload\bin\ottrin-indexd.exe" '--stop'}}catch{$result.cleanup_error=$_.Exception.Message;$result.status='fail'}}
 if($installed){try{if(Test-Path "$install\bin\ottrin-indexd.exe"){$null=Process "$install\bin\ottrin-indexd.exe" '--stop'};if($Mode-eq'MSI'-and$code){Msi "/x $code" 'recovery-uninstall'}elseif(Test-Path "$install\Uninstall.exe"){$null=Process "$install\Uninstall.exe" '/S';WaitUninstalled}}catch{$result.cleanup_error=$_.Exception.Message;$result.status='fail'}}
 foreach($p in $owned){if(-not$p.HasExited){& "$env:WINDIR\System32\taskkill.exe" /PID $p.Id /T /F|Out-Null;if(-not$p.WaitForExit(10000)){$result.status='fail';$result.cleanup_error='Owned process remains'}}}
 try{if(Test-Path $work){Remove-Item -LiteralPath $work -Recurse -Force}}catch{$result.cleanup_error=$_.Exception.Message;$result.status='fail'};$result|ConvertTo-Json -Depth 10|Set-Content "$r\CONTENT_$Mode.json" -Encoding UTF8
}
if($result.status-ne'pass'){exit 1}
