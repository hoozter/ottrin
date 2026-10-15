$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'
$b='C:\OttrinTreeUIQA'; $r="$b\results"
function Assert($value,$message) { if (-not $value) { throw $message } }
. "$b\snapshot.ps1"
try {
    Assert ($env:USERNAME -eq 'OttrinTreeUIQA') 'Wrong account'
    $report=Get-Content "$r\RESULTS.json" -Raw -Encoding UTF8 | ConvertFrom-Json
    Assert ($report.status -eq 'pass' -and @($report.cases).Count -eq 20) 'Expected twenty accepted cases'
    $verified=@()
    foreach ($case in $report.cases) { $verified+=VerifyCase $case }
    Assert (-not (Test-Path "$env:LOCALAPPDATA\Programs\Ottrin")) 'Normal uninstall left product files'
    $scheduler=New-Object -ComObject Schedule.Service; $scheduler.Connect()
    $name='Ottrin Index '+[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    $absent=$false
    try { $null=$scheduler.GetFolder('\').GetTask($name) } catch { if ($_.Exception.GetBaseException().HResult -eq -2147024894) { $absent=$true } else { throw } }
    Assert $absent 'Own index task remains after uninstall'
    @{status='pass'; cases=$verified; index_task_absent=$true; scope='All twenty directory/mixed-batch outcomes independently rechecked after normal GUI close and normal uninstall'} | ConvertTo-Json -Depth 20 | Set-Content "$r\FINAL_STATE.json" -Encoding UTF8
} catch { @{status='fail'; error=$_.Exception.Message; stack=$_.ScriptStackTrace} | ConvertTo-Json | Set-Content "$r\FINAL_STATE.json" -Encoding UTF8; exit 1 }
