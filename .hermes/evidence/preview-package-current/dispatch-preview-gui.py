"""Run the ordinary-user portable preview GUI actor in the owned COW guest."""
from pathlib import Path
import subprocess,sys,tempfile,time
p=Path(__file__).resolve().parent;sys.path.insert(0,str(p));import transport as n
from fixtures import create
assert n.execute("if((Get-ScheduledTask 'Ottrin Content QA').State-eq'Running'){throw 'Actor live'}")==0
with tempfile.TemporaryDirectory(prefix='op-gui-input-',dir='.hermes/qa') as temporary:
 for source in create(Path(temporary)):n.upload(source,source.name)
for name in ['windows-preview-gui.ps1','gui-runner.ps1','gui-config.json']:n.upload(p/name,name)
assert n.execute(r"$action=New-ScheduledTaskAction -Execute C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe -Argument '-NoProfile -ExecutionPolicy Bypass -File C:\OttrinContentQA\gui-runner.ps1';Set-ScheduledTask -TaskName 'Ottrin Content QA' -Action $action|Out-Null;Start-ScheduledTask 'Ottrin Content QA'")==0
# Real pointer input is sent only to the XML/volume/name-verified guest VNC port.
# Wait for the actor's app before dismissing first-login shell UI over its title.
deadline=time.monotonic()+30
while True:
 pid=n.start(r"$app=Get-Process ottrin -ErrorAction SilentlyContinue|Where-Object {$_.Path-like'C:\OttrinContentQA\payload\*'};if(-not$app){exit 2}")
 result=n.qga('guest-exec-status',{'pid':pid})
 while not result.get('exited'):
  time.sleep(.2);result=n.qga('guest-exec-status',{'pid':pid})
 if result.get('exitcode')==0:break
 if time.monotonic()>deadline:raise TimeoutError('Owned GUI has not appeared; inspect actor, do not relaunch')
 time.sleep(.5)
time.sleep(.5);subprocess.run([sys.executable,str(p/'dismiss-guest-menu.py'),'--app-live'],check=True)
print('Owned GUI actor dispatched; inspect scheduled task and GUI_PREVIEWS.json for terminal result')
