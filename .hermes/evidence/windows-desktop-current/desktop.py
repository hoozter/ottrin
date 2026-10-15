import pathlib,sys,json,time
r=pathlib.Path.cwd();sys.path.insert(0,str(r/'.hermes/evidence/windows-package-current'));import native_compiler as n;n.GROOT=r'C:\OttrinDesktopQA'
if sys.argv[1:] == ['setup']:
 n.upload(r/'.hermes/qa/windows-desktop-current/desktop.ps1','desktop.ps1')
 assert n.execute(r'''$a=New-ScheduledTaskAction -Execute 'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe' -Argument '-NoProfile -NonInteractive -ExecutionPolicy Bypass -File C:\OttrinDesktopQA\desktop.ps1';$p=New-ScheduledTaskPrincipal -UserId "$env:COMPUTERNAME\OttrinDesktopQA" -LogonType Interactive -RunLevel Limited;Register-ScheduledTask -TaskName 'Ottrin Desktop Input QA' -Action $a -Principal $p|Out-Null''')==0
 sys.exit()
c={'op':'observe'} if len(sys.argv)==1 else {'op':'click','x':int(sys.argv[1]),'y':int(sys.argv[2])}
p=r/'.hermes/qa/windows-desktop-current/desktop-command.json';p.write_text(json.dumps(c));n.upload(p,'desktop-command.json')
assert n.execute(r'''$s=New-Object -ComObject Schedule.Service;$s.Connect();$t=$s.GetFolder('\').GetTask('Ottrin Desktop Input QA');if($t.State-eq4){throw 'Input actor already running'};$null=$t.Run($null)''')==0
for attempt in range(50):
 # Poll task state through one guest process without starting a second actor.
 code=n.execute(r'''$s=New-Object -ComObject Schedule.Service;$s.Connect();$t=$s.GetFolder('\').GetTask('Ottrin Desktop Input QA');if($t.State-eq4){exit 7};if($t.LastTaskResult-ne0){throw "Input actor exit $($t.LastTaskResult)"}''')
 if code==0:break
 if code!=7:raise RuntimeError('Input actor failed')
 time.sleep(.2)
else:raise TimeoutError('Input actor remains live; poll its task')
n.download('results\\desktop.png',r/'.hermes/qa/windows-desktop-current/desktop.png')
