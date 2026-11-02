param([Parameter(Mandatory=$true)][string]$Media)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$root='C:\OttrinStockClient'
New-Item -ItemType Directory -Path $root -Force|Out-Null
$result=@{status='validating bootstrap';security='Normal Windows driver/installer enforcement; no UAC, antivirus, test-signing or account requirement bypass';steps=@()}
function Save {
 $path=Join-Path $root 'BOOTSTRAP.json';$temp=$path+'.tmp'
 try{[IO.File]::WriteAllText($temp,($result|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false));if([IO.File]::Exists($path)){[IO.File]::Replace($temp,$path,[NullString]::Value)}else{[IO.File]::Move($temp,$path)}}finally{if([IO.File]::Exists($temp)){[IO.File]::Delete($temp)}}
}
try {
 $input=Get-Content (Join-Path $Media 'bootstrap-input.json') -Raw|ConvertFrom-Json;$result.nonce=$input.nonce;Save
 foreach($item in $input.files){$path=Join-Path $Media $item.path;if((Get-FileHash $path).Hash.ToLowerInvariant()-ne$item.sha256){throw 'Bootstrap payload hash mismatch'}}
 $catalog=Join-Path $Media 'vioserial\w11\amd64\vioser.cat';$signature=Get-AuthenticodeSignature $catalog
 $result.driver_signature=@{status=[string]$signature.Status;subject=[string]$signature.SignerCertificate.Subject;thumbprint=[string]$signature.SignerCertificate.Thumbprint};Save
 if($signature.Status-ne'Valid'){throw 'Virtio serial kernel driver catalog is not trusted by normal Windows verification'}
 & "$env:SystemRoot\System32\pnputil.exe" /add-driver (Join-Path $Media 'vioserial\w11\amd64\vioser.inf') /install 2>&1|Set-Content (Join-Path $root 'PNP.log');$result.driver_exit=$LASTEXITCODE;Save
 if($LASTEXITCODE-notin@(0,3010)){throw 'Normal signed serial driver installation failed'}
 $msi=Join-Path $Media 'guest-agent\qemu-ga-x86_64.msi';$signature=Get-AuthenticodeSignature $msi
 $result.agent_signature=@{status=[string]$signature.Status;subject=[string]$signature.SignerCertificate.Subject;thumbprint=[string]$signature.SignerCertificate.Thumbprint};Save
 # Community userspace tools can be unsigned. Their exact upstream HTTPS bytes
 # are bound above; Windows retains its normal installer enforcement.
 if([string]$signature.Status-notin@('Valid','NotSigned')){throw 'Guest agent has an invalid or untrusted signature'}
 $p=Start-Process "$env:SystemRoot\System32\msiexec.exe" -ArgumentList @('/i',('"'+$msi+'"'),'/qn','/norestart','/l*v',('"'+(Join-Path $root 'AGENT.log')+'"')) -PassThru
 if(-not$p.WaitForExit(120000)){$p.Kill();throw 'Owned guest-agent install timed out'};$p.WaitForExit();$result.agent_install_exit=$p.ExitCode;Save
 if($p.ExitCode-notin@(0,3010)){throw 'Guest-agent MSI failed'}
 $service=Get-Service QEMU-GA -ErrorAction Stop;if($service.Status-ne'Running'){Start-Service $service};$result.agent_service=(Get-Service QEMU-GA).Status.ToString()
 $winlogon='HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon';Set-ItemProperty $winlogon AutoAdminLogon '0';Remove-ItemProperty $winlogon DefaultPassword -ErrorAction SilentlyContinue
 $result.autologon_removed=$true;$result.windows=@{caption=(Get-CimInstance Win32_OperatingSystem).Caption;version=(Get-CimInstance Win32_OperatingSystem).Version;build=(Get-CimInstance Win32_OperatingSystem).BuildNumber;msi=(Get-Item "$env:SystemRoot\System32\msi.dll").VersionInfo.FileVersion}
 $result.status='ready for independent MSI comparison';Save
} catch { $result.status='failed bootstrap';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace;Save;exit 1 }
