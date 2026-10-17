$ErrorActionPreference='Stop'
$r='C:\OttrinCopyCleanupQA\office-macro';$p=$null;$report=@{status='running';cases=@()}
try {
 if($env:USERNAME-ne'OttrinCopyQA'){throw 'Wrong control account'}
 $id=[Security.Principal.WindowsIdentity]::GetCurrent();if(([Security.Principal.WindowsPrincipal]::new($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Elevated control'}
 if(Test-Path "$r\work"){throw 'Control workspace occupied'};New-Item -ItemType Directory "$r\work"|Out-Null
 foreach($name in @('export')) {
  $dir="$r\work\$name";New-Item -ItemType Directory $dir|Out-Null
  $input="$r\$name.odt";$before=(Get-FileHash $input).Hash;$profile=([Uri]"$dir\profile").AbsoluteUri
  $p=Start-Process "$r\denied.exe" -ArgumentList @('"C:\Program Files\LibreOffice\program"',"-env:UserInstallation=$profile",$input,$dir) -PassThru -WorkingDirectory $dir -RedirectStandardOutput "$r\denied.stdout" -RedirectStandardError "$r\denied.stderr"
  $null=$p.Handle;$timeout=-not$p.WaitForExit(35000)
  if($timeout){& C:\Windows\System32\taskkill.exe /PID $p.Id /T /F|Out-Null;if(-not$p.WaitForExit(10000)){throw 'Control process did not stop'}}
  if((Get-FileHash $input).Hash-ne$before){throw 'Control source changed'}
  $converted=(-not$timeout)-and($p.ExitCode-eq0)-and(Test-Path "$dir\kit-result.pdf")
  if($converted){Copy-Item "$dir\kit-result.pdf" "$r\denied.pdf"}
  $report.cases+=@{name=$name;converted=$converted;timeout=$timeout;exit=$p.ExitCode;source_sha256=$before};$p=$null
 }
 $report.status='pass'
} catch {$report.status='fail';$report.error=$_.Exception.ToString()}
finally {
 if($p-and-not$p.HasExited){& C:\Windows\System32\taskkill.exe /PID $p.Id /T /F|Out-Null;$null=$p.WaitForExit(10000)}
 $report.owned_processes=@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-eq"$r\denied.exe"}).Count
 if($report.owned_processes){$report.status='fail'}elseif(Test-Path "$r\work"){Remove-Item -LiteralPath "$r\work" -Recurse -Force}
 $report.workspace_removed=(-not(Test-Path "$r\work"));$report|ConvertTo-Json -Depth 5|Set-Content "$r\CONTROL_DENIED.json"
}
if($report.status-ne'pass'){exit 1}
