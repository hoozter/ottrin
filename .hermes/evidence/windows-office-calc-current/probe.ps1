$ErrorActionPreference='Stop'
$root='C:\OttrinCopyCleanupQA'
$work=Join-Path $root 'office-probe'
$results=Join-Path $root 'results'
$process=$null
$report=@{status='running';cases=@()}
try {
 if($env:USERNAME-ne'OttrinCopyQA'){throw 'Wrong actor'}
 $identity=[Security.Principal.WindowsIdentity]::GetCurrent()
 if(([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Elevated actor'}
 if(Test-Path $work){throw 'Owned workspace already exists'}
 New-Item -ItemType Directory $work|Out-Null
 foreach($case in @(@{name='writer-default';file='document.docx';plugin=''},@{name='calc-default';file='sheet.xlsx';plugin=''},@{name='calc-bitmap';file='sheet.xlsx';plugin='svp'},@{name='impress-default';file='slides.pptx';plugin=''},@{name='impress-bitmap';file='slides.pptx';plugin='svp'})) {
  $dir=Join-Path $work $case.name;New-Item -ItemType Directory $dir|Out-Null
  $profile=Join-Path $dir 'profile';$output=Join-Path $dir 'output';New-Item -ItemType Directory $output|Out-Null
  $input=Join-Path $root $case.file;$before=(Get-FileHash $input).Hash
  if($case.plugin){$env:SAL_USE_VCLPLUGIN=$case.plugin}else{Remove-Item Env:SAL_USE_VCLPLUGIN -ErrorAction SilentlyContinue}
  $env:SAL_LOG='+INFO.vcl.plugadapt+WARN'
  $arguments=@('"C:\Program Files\LibreOffice\program"',('-env:UserInstallation='+([Uri]$profile).AbsoluteUri),$input,$output)
  $watch=[Diagnostics.Stopwatch]::StartNew()
  $process=Start-Process "$root\office-probe.exe" -ArgumentList $arguments -WorkingDirectory $dir -PassThru -RedirectStandardOutput "$results\office-$($case.name).stdout" -RedirectStandardError "$results\office-$($case.name).stderr"
  $null=$process.Handle
  $timeout=-not$process.WaitForExit(30000)
  if($timeout){& C:\Windows\System32\taskkill.exe /PID $process.Id /T /F|Out-Null;if(-not$process.WaitForExit(10000)){throw 'Owned helper did not stop'}}
  $watch.Stop();$process.Refresh();$exit=$process.ExitCode
  if((Get-FileHash $input).Hash-ne$before){throw 'Fixture source changed'}
  $pdf=Join-Path $output 'source.pdf';$converted=(-not$timeout)-and($exit-eq0)-and(Test-Path $pdf)
  if($converted){Copy-Item $pdf "$results\office-$($case.name).pdf"}
  $report.cases+=@{name=$case.name;timeout=$timeout;exit=$exit;converted=$converted;elapsed_ms=$watch.ElapsedMilliseconds;input_sha256=$before}
  $process=$null
 }
 $report.status='pass'
} catch {$report.status='fail';$report.error=$_.Exception.ToString()}
finally {
 if($process-and-not$process.HasExited){& C:\Windows\System32\taskkill.exe /PID $process.Id /T /F|Out-Null;$process.WaitForExit(10000)|Out-Null}
 $live=@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-eq"$root\office-probe.exe"}).Count
 $report.owned_helpers=$live
 if($live){$report.status='fail'}else{if(Test-Path $work){Remove-Item -LiteralPath $work -Recurse -Force};$report.workspace_removed=(-not(Test-Path $work))}
 $report|ConvertTo-Json -Depth 5|Set-Content "$results\OFFICE_PROBE.json"
}
if($report.status-ne'pass'){exit 1}
