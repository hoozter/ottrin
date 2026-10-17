from pathlib import Path
import sys,json
p=Path(__file__).resolve().parent
sys.path.insert(0,str(p.parent/'windows-copy-cleanup-current'));import transport as n
n.GROOT=r'C:\OttrinCopyCleanupQA\desktop'
if sys.argv[1:] == ['setup']:
 n.upload(p/'desktop.ps1','desktop.ps1')
 pid=n.start(r'''if((Get-CimInstance Win32_ComputerSystem).UserName-notlike'*\OttrinCopyQA'){throw 'Owned console missing'};if(Get-ScheduledTask 'Ottrin Copy Activity Desktop QA' -ErrorAction SilentlyContinue){throw 'Desktop actor already registered'};$a=New-ScheduledTaskAction -Execute 'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe' -Argument '-NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File C:\OttrinCopyCleanupQA\desktop\desktop.ps1';$principal=New-ScheduledTaskPrincipal -UserId "$env:COMPUTERNAME\OttrinCopyQA" -LogonType Interactive -RunLevel Limited;Register-ScheduledTask -TaskName 'Ottrin Copy Activity Desktop QA' -Action $a -Principal $principal|Out-Null''');n.wait(pid,45);raise SystemExit()
if sys.argv[1:] != ['resume']:
 command=json.loads(sys.argv[1]);(p/'command.json').write_text(json.dumps(command));n.upload(p/'command.json','command.json')
 pid=n.start(r'''if((Get-CimInstance Win32_ComputerSystem).UserName-notlike'*\OttrinCopyQA'){throw 'Wrong console user'};$t=Get-ScheduledTask 'Ottrin Copy Activity Desktop QA';if($t.State.ToString()-in@('Running','Queued')){throw 'Owned desktop actor active'};Remove-Item C:\OttrinCopyCleanupQA\desktop\results\actor-error.txt -ErrorAction SilentlyContinue;Start-ScheduledTask $t.TaskName;do{Start-Sleep -Milliseconds 150;$t=Get-ScheduledTask $t.TaskName}while($t.State.ToString()-in@('Running','Queued'));if((Get-ScheduledTaskInfo $t.TaskName).LastTaskResult-ne0){throw 'Owned desktop actor failed'}''')
 (p/'observer.pid').write_text(str(pid))
else:pid=int((p/'observer.pid').read_text())
try:n.wait(pid,45)
except TimeoutError:
 print('Desktop observer remains live; resume same PID',flush=True);raise SystemExit(75)
except AssertionError:
 try:n.download('results\\actor-error.txt',p/'actor-error.txt');print((p/'actor-error.txt').read_text(encoding='utf-8-sig'))
 except Exception:pass
 raise
for name in ['desktop.png','desktop.json']:n.download('results\\'+name,p/name)
print((p/'desktop.json').read_text(encoding='utf-8-sig'))
