from pathlib import Path
import sys,json
p=Path(__file__).resolve().parent;sys.path.insert(0,str(p));import transport as n
for actor,prefix in [('policy-actor.ps1','POLICY'),('junction-policy-actor.ps1','JUNCTION_POLICY')]:
 n.upload(p/actor,actor)
 script=r"""if((Get-ScheduledTask 'Ottrin Content QA').State-eq'Running'){throw 'Same actor still live'};$mode=(Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock').AllowDevelopmentWithoutDevLicense;if($mode-ne0){throw 'Policy lane requires disabled Developer Mode'};$action=New-ScheduledTaskAction -Execute C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe -Argument '-NoProfile -ExecutionPolicy Bypass -File C:\OttrinContentQA\ACTOR';Set-ScheduledTask -TaskName 'Ottrin Content QA' -Action $action|Out-Null;Start-ScheduledTask 'Ottrin Content QA';$until=(Get-Date).AddSeconds(45);do{Start-Sleep -Milliseconds 250;$state=(Get-ScheduledTask 'Ottrin Content QA').State}while($state-eq'Running'-and(Get-Date)-lt$until);if($state-eq'Running'){throw 'Same scheduled actor remains live; poll it before any retry'};$result=(Get-ScheduledTaskInfo 'Ottrin Content QA').LastTaskResult;if($result-ne0){throw "Policy actor result: $result"}""".replace('ACTOR',actor)
 pid=n.start(script);(p/(prefix+'_PID.json')).write_text(json.dumps({'pid':pid})+'\n');n.wait(pid,seconds=50)
 for name in ['platform.stdout','platform.stderr','RESULTS.json']:n.download('results\\'+name,p/(prefix+'_'+name))
 print((p/(prefix+'_RESULTS.json')).read_text());print((p/(prefix+'_platform.stdout')).read_text());print((p/(prefix+'_platform.stderr')).read_text())
 assert 'test result: ok. 1 passed; 0 failed; 0 ignored' in (p/(prefix+'_platform.stdout')).read_text()
 assert json.loads((p/(prefix+'_RESULTS.json')).read_text())['status']=='pass'
