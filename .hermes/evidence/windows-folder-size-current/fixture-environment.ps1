$path='HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock'
$previous=[Microsoft.Win32.Registry]::GetValue('HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock','AllowDevelopmentWithoutDevLicense',$null)
New-Item $path -Force|Out-Null
New-ItemProperty $path -Name AllowDevelopmentWithoutDevLicense -Value 1 -PropertyType DWord -Force|Out-Null
@{scope='Owned COW guest only; symlink test fixtures';previous=$previous;cleanup='Whole overlay disposal'}|ConvertTo-Json|Set-Content C:\OttrinContentQA\results\FIXTURE_ENVIRONMENT.json
$t=Get-ScheduledTask 'Ottrin Content QA';if($t.State-eq'Running'){throw 'Actor remains live'}
Remove-Item C:\OttrinContentQA\results\RESULTS.json
Start-ScheduledTask $t.TaskName
(Get-ScheduledTask $t.TaskName).State.ToString()
