$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$b='C:\OttrinQA-20261011';$r="$b\office-loop-current3"
try {
 $results=@()
 foreach($mode in @('sheet.xls','slides.odp')) {
  $work="$r\$mode";New-Item -ItemType Directory "$work\profile","$work\output" -ErrorAction Stop|Out-Null
  $original="$b\office-formats\$mode";$input="$work\$mode";Copy-Item $original $input;$before=(Get-FileHash $input).Hash
  $watch=[Diagnostics.Stopwatch]::StartNew()
  $p=Start-Process -FilePath "$b\office-loop-pump.exe" -WorkingDirectory $work -ArgumentList @("-env:UserInstallation=file:///C:/OttrinQA-20261011/office-loop-current3/$mode/profile", "$work\output", $input) -PassThru -RedirectStandardOutput "$r\$mode.out" -RedirectStandardError "$r\$mode.err"
  $handle=$p.Handle
  try {if(-not $p.WaitForExit(40000)){Stop-Process -Id $p.Id -Force;throw 'Parent probe deadline expired'};$p.WaitForExit();$code=$p.ExitCode;if($null -eq $code){throw "Exit code unavailable"}} finally {if(-not $p.HasExited){Stop-Process -Id $p.Id -Force}}
  if((Get-FileHash $input).Hash -ne $before){throw 'source changed'}
  $results+=@{mode=$mode;exit=$code;elapsed_ms=$watch.ElapsedMilliseconds;source_unchanged=$true;pdf=(Test-Path "$work\output\kit-result.pdf");exe_sha256=(Get-FileHash "$b\office-loop-pump.exe").Hash;identity=(& whoami);groups=(& whoami /groups | Out-String)}
 }
 $results|ConvertTo-Json -Depth 5|Set-Content "$r\results.json"
} catch {$_|Out-String|Set-Content "$r\failure.txt";exit 1}
