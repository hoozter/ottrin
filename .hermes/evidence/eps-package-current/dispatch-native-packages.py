from pathlib import Path
import json,hashlib,sys
p=Path(__file__).resolve().parent;sys.path.insert(0,str(p));import transport as n
stage=Path('.hermes/packaging-tmp/eps-refresh/windows')
manifest=json.loads((p/'FINAL_ARCHIVE_CONTENTS.json').read_text())['payload_sha256'];(p/'payload.json').write_text(json.dumps(manifest)+'\n');n.upload(p/'payload.json','payload.json');n.upload(p/'content-actor.ps1','content-actor.ps1');n.upload(p/'admin-extract.ps1','admin-extract.ps1');n.upload(stage/'Ottrin-0.1.0-windows-x64-setup.exe','setup.exe')
pins={'setup.exe':hashlib.sha256((stage/'Ottrin-0.1.0-windows-x64-setup.exe').read_bytes()).hexdigest(),'current.msi':hashlib.sha256((stage/'Ottrin-0.1.0-windows-x64.msi').read_bytes()).hexdigest()};(p/'NATIVE_INSTALLER_INPUTS.json').write_text(json.dumps(pins,indent=2)+'\n')
assert n.execute(";".join("if((Get-FileHash 'C:\\OttrinContentQA\\"+name+"').Hash.ToLowerInvariant()-ne'"+digest+"'){throw 'Installer input differs'}" for name,digest in pins.items()))==0
script=r'''$tokens=$null;$errors=$null;[Management.Automation.Language.Parser]::ParseFile('C:\OttrinContentQA\content-actor.ps1',[ref]$tokens,[ref]$errors)|Out-Null;if($errors.Count){throw ($errors|Out-String)}
foreach($mode in @('Portable','NSIS','MSI')){
 if((Get-ScheduledTask 'Ottrin Content QA').State-eq'Running'){throw 'Task already live'}
 $prior=(Get-ScheduledTaskInfo 'Ottrin Content QA').LastRunTime
 $action=New-ScheduledTaskAction -Execute C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe -Argument "-NoProfile -ExecutionPolicy Bypass -File C:\OttrinContentQA\content-actor.ps1 -Mode $mode"
 Set-ScheduledTask -TaskName 'Ottrin Content QA' -Action $action|Out-Null;Start-ScheduledTask 'Ottrin Content QA'
 $until=[DateTime]::UtcNow.AddMinutes(4)
 do {Start-Sleep -Milliseconds 400;$task=Get-ScheduledTask 'Ottrin Content QA';$info=Get-ScheduledTaskInfo 'Ottrin Content QA';if([DateTime]::UtcNow-gt$until){throw "Task remains live or unstarted: $mode"}}while($info.LastRunTime-le$prior-or$task.State-eq'Running')
 if($info.LastTaskResult-ne0){throw "Package actor failed $mode : $($info.LastTaskResult)"}
 $r=Get-Content "C:\OttrinContentQA\results\CONTENT_$mode.json" -Raw|ConvertFrom-Json;if($r.status-ne'pass'){throw "Actor result failed $mode"};"$mode passed"
}
& C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\OttrinContentQA\admin-extract.ps1
if($LASTEXITCODE-ne0){throw 'Administrative extraction failed'}
'''
pid=n.start(script);(p/'NATIVE_CASES_PID.json').write_text(json.dumps({'pid':pid})+'\n');print('Native packaged cases dispatched:',pid,flush=True)
