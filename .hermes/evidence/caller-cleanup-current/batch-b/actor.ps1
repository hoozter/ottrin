param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExpectedSid)
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$out=Join-Path $Root 'results'
$result=@{status='running';phase='waiting for owned desktop';runs=@();scope='Ordinary owned actor coordinates a disjoint SYSTEM native cleanup regression batch; no caller/UAC/package acceptance'}
function Save{[IO.File]::WriteAllText((Join-Path $out 'RESULT.json'),($result|ConvertTo-Json -Depth 18),[Text.UTF8Encoding]::new($false))}
function Require($value,$message){if(-not$value){throw $message}}
Save
try {
 $id=[Security.Principal.WindowsIdentity]::GetCurrent();Require ($id.User.Value-eq$ExpectedSid) 'Fresh owned actor required'
 Require (-not([Security.Principal.WindowsPrincipal]::new($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) 'Ordinary actor required'
 Require ((Get-Process -Id $PID).SessionId-gt0) 'Interactive owned session required'
 $ready=Join-Path $out 'DESKTOP_READY.json';$deadline=[DateTime]::UtcNow.AddMinutes(8)
 while(-not(Test-Path $ready)){Require ([DateTime]::UtcNow-lt$deadline) 'Owned desktop timed out';Start-Sleep -Milliseconds 200}
 Require ((Get-Content $ready -Raw|ConvertFrom-Json).sid-eq$ExpectedSid) 'Owned desktop SID mismatch'
 [IO.File]::WriteAllText((Join-Path $out 'CALLER_READY.json'),(@{pid=$PID;sid=$ExpectedSid;role='ordinary fixture coordinator'}|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
 $result.phase='waiting for individually recorded SYSTEM cleanup tests';Save
 $done=Join-Path $out 'SYSTEM_RESULT.json';$deadline=[DateTime]::UtcNow.AddMinutes(18)
 while(-not(Test-Path $done)){Require ([DateTime]::UtcNow-lt$deadline) 'SYSTEM batch result timed out';Start-Sleep -Milliseconds 200}
 $system=Get-Content $done -Raw|ConvertFrom-Json;$result.system=$system;Require ($system.status.StartsWith('pass')) 'SYSTEM cleanup batch failed'
 $result.status='pass disjoint native cleanup regression batch';$result.phase='complete'
}catch{$result.status='failed';$result.error=$_.Exception.Message;$result.stack=$_.ScriptStackTrace}
finally{Save}
if(-not$result.status.StartsWith('pass')){exit 1}
