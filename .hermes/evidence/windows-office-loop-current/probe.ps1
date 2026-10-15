$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$b='C:\OttrinQA-20261011';$r="$b\office-loop-current"
try {
 $results=@()
 foreach($mode in @('control','pump')) {
  $work="$r\$mode";New-Item -ItemType Directory "$work\profile","$work\output" -ErrorAction Stop|Out-Null
  $input="$b\office-formats\sheet.xls";$before=(Get-FileHash $input).Hash
  $watch=[Diagnostics.Stopwatch]::StartNew()
  $p=Start-Process -FilePath "$b\office-loop-$mode.exe" -WorkingDirectory $work -ArgumentList @("-env:UserInstallation=file:///C:/OttrinQA-20261011/office-loop-current/$mode/profile", "$work\output", $input) -PassThru -RedirectStandardOutput "$r\$mode.out" -RedirectStandardError "$r\$mode.err"
  try {if(-not $p.WaitForExit(40000)){Stop-Process -Id $p.Id -Force;throw 'Parent probe deadline expired'};$p.Refresh();$code=$p.ExitCode} finally {if(-not $p.HasExited){Stop-Process -Id $p.Id -Force}}
  if((Get-FileHash $input).Hash -ne $before){throw 'source changed'}
  $results+=@{mode=$mode;exit=$code;elapsed_ms=$watch.ElapsedMilliseconds;source_unchanged=$true;pdf=(Test-Path "$work\output\kit-result.pdf");exe_sha256=(Get-FileHash "$b\office-loop-$mode.exe").Hash;identity=(& whoami);groups=(& whoami /groups | Out-String)}
 }
 $results|ConvertTo-Json -Depth 5|Set-Content "$r\results.json"
} catch {$_|Out-String|Set-Content "$r\failure.txt";exit 1}
