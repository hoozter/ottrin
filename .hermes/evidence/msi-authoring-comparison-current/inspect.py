from pathlib import Path
import json,hashlib,sys,time
p=Path(__file__).resolve().parent;root=p.parents[2]
sys.path.insert(0,str(p.parent/'windows-copy-cleanup-current'));import transport as n
n.GROOT=r'C:\OttrinCopyCleanupQA\msi-authoring-compare'
def wait(script,phase):
 pid=n.start(script);(p/(phase+'.pid')).write_text(str(pid))
 while True:
  try:return n.wait(pid,45)
  except TimeoutError:print('Continue observing same '+phase+' PID',flush=True)
end=time.monotonic()+120
while True:
 try:n.qga('guest-ping',{});break
 except Exception:
  if time.monotonic()>end:raise
  time.sleep(1)
wait(r'''$r='C:\OttrinCopyCleanupQA\msi-authoring-compare';if(Test-Path $r){throw 'Owned comparison root already exists'};if(@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-like'C:\OttrinCopyCleanupQA\*'}).Count){throw 'Owned QA process remains'};New-Item -ItemType Directory $r|Out-Null
$b=@{};foreach($path in @('C:\Users\campbell\AppData\Roaming\ottrin\config.json','C:\Users\campbell\AppData\Local\Programs\Ottrin\Uninstall.exe')){$b[$path]=$(if(Test-Path -LiteralPath $path){(Get-FileHash -LiteralPath $path).Hash}else{$null})};$b|ConvertTo-Json|Set-Content "$r\BASELINE.json"
''','prepare')
n.download('BASELINE.json',p/'BASELINE.json')
inputs=json.loads((p/'INPUTS.json').read_text())
for name in inputs:n.upload(p/'inputs'/(name+'.msi'),name+'.msi')
current=root/'.hermes/packaging-tmp/windows-activity-candidate/Ottrin-0.1.0-windows-x64.msi'
inputs['ottrin']={'path':str(current),'msi_sha256':hashlib.sha256(current.read_bytes()).hexdigest(),'msi_bytes':current.stat().st_size,'source_commit':'566f6236aaf29cc95a3548b22cfb6da5e116a94f'}
(p/'INPUTS.json').write_text(json.dumps(inputs,indent=2)+'\n');n.upload(current,'ottrin.msi');n.upload(p/'INPUTS.json','INPUTS.json')
wait((p/'inspect.ps1').read_text(),'inspect')
n.download('TABLES.json',p/'TABLES.json')
