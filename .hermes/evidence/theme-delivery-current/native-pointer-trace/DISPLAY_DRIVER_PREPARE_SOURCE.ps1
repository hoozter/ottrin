param([Parameter(Mandatory=$true)][string]$Output,[Parameter(Mandatory=$true)][string]$SourceDirectory)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
if([Security.Principal.WindowsIdentity]::GetCurrent().User.Value-ne'S-1-5-18'){throw 'Driver setup requires the owned clone SYSTEM bootstrap'}
if(Test-Path $Output){throw 'Existing display-driver fixture retained'}
$null=[IO.Directory]::CreateDirectory($Output)
$driverInput=Get-Content (Join-Path $SourceDirectory 'display-driver-input.json') -Raw|ConvertFrom-Json
$files=@($driverInput.files.PSObject.Properties);if($files.Count-ne3){throw 'Expected exact three-file display driver'}
foreach($file in $files){if($file.Name-notin@('viogpudo.inf','viogpudo.cat','viogpudo.sys')){throw 'Unexpected driver member'};$source=Join-Path $SourceDirectory $file.Name;if((Get-FileHash $source).Hash.ToLowerInvariant()-ne$file.Value){throw ('Driver hash mismatch '+$file.Name)};Copy-Item $source (Join-Path $Output $file.Name)}
$catalog=Get-AuthenticodeSignature (Join-Path $Output 'viogpudo.cat');if($catalog.Status-ne'Valid'-or$catalog.SignerCertificate.Subject-notlike'*Microsoft*'){throw ('Native Microsoft catalog trust failed: '+$catalog.Status+' '+$catalog.StatusMessage)}
$before=@(Get-CimInstance Win32_VideoController|Select-Object Name,PNPDeviceID,DriverVersion,CurrentHorizontalResolution,CurrentVerticalResolution)
$exe=Join-Path $env:SystemRoot 'System32\pnputil.exe';$inf=Join-Path $Output 'viogpudo.inf';$process=Start-Process $exe -ArgumentList @('/add-driver',('"'+$inf+'"'),'/install') -PassThru -RedirectStandardOutput (Join-Path $Output 'pnputil.stdout.log') -RedirectStandardError (Join-Path $Output 'pnputil.stderr.log');$null=$process.Handle
try{if(-not$process.WaitForExit(90000)){throw 'Owned display-driver install timed out'};$process.WaitForExit();$code=$process.ExitCode;if($code-notin@(0,3010)){throw ('Native signed-driver installation failed '+$code)}}finally{if(-not$process.HasExited){$process.Kill();$null=$process.WaitForExit(10000)};$process.Dispose()}
$after=@(Get-CimInstance Win32_PnPSignedDriver|Where-Object {$_.DeviceClass-eq'DISPLAY'}|Select-Object DeviceName,DeviceID,DriverVersion,DriverProviderName,InfName,IsSigned,Signer)
$receipt=@{status='native Microsoft catalog validated and pnputil completed; rebooted runtime modes remain separate';input=$driverInput;catalog_subject=$catalog.SignerCertificate.Subject;catalog_thumbprint=$catalog.SignerCertificate.Thumbprint;before=$before;after=$after;pnputil_exit=$code;stdout=[IO.File]::ReadAllText((Join-Path $Output 'pnputil.stdout.log'));stderr=[IO.File]::ReadAllText((Join-Path $Output 'pnputil.stderr.log'));scope='Only the disposable offline clone; no certificate import, test-signing, policy/ACL change, host or retained base mutation'}
[IO.File]::WriteAllText((Join-Path $Output 'RESULT.json'),($receipt|ConvertTo-Json -Depth 12),[Text.UTF8Encoding]::new($false));$receipt|ConvertTo-Json -Depth 12 -Compress
