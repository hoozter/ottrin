"""Extend only the owned QA actor for a cooperating caller's picker launch and idle sample."""
from pathlib import Path
import hashlib,json,base64
import native_compiler as n
r=Path.cwd();e=Path(__file__).resolve().parent;g=e.parent/'windows-picker-gui-current'
session=json.loads((g/'SESSION.json').read_text());assert session['status']=='waiting for owned input'
canonical=r/'scripts/windows-desktop-qa/actor.ps1';source=canonical.read_text();old_sha=hashlib.sha256(canonical.read_bytes()).hexdigest()
needle="}elseif($c.op-eq'launch'){";assert source.count(needle)==1
source=source.replace(needle,r'''}elseif($c.op-eq'picker-idle'){
 if($processes.Count-ne1-or$processes[0].CommandLine-notmatch'\spicker\s'){throw 'One owned picker is required'}
 $observed=Get-Process -Id $processes[0].ProcessId;$before=$observed.TotalProcessorTime.TotalMilliseconds;$started=[Diagnostics.Stopwatch]::StartNew();Start-Sleep -Seconds 10;$observed.Refresh();$elapsed=$started.Elapsed.TotalSeconds
 $cpu=($observed.TotalProcessorTime.TotalMilliseconds-$before)/10/$elapsed
 $verification=@{status='pass';seconds=$elapsed;cpu_percent_one_core=$cpu;working_set_bytes=$observed.WorkingSet64;scope='Owned ordinary-account OpenGL picker; bounded quiet interval'}
 if($cpu-gt2){throw 'Investigate sustained chooser CPU before acceptance'}
 if(@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath-eq"$root\payload\Ottrin-0.1.0-windows-x64\bin\ottrin-indexd.exe"}).Count){throw 'Standalone picker started an index'}
}elseif($c.op-in@('launch','launch-picker')){''')
needle=' $p=Start-Process $app -ArgumentList $arguments -WorkingDirectory (Split-Path $app) -PassThru -RedirectStandardOutput "$root\\results\\app.stdout" -RedirectStandardError "$root\\results\\app.stderr"';assert source.count(needle)==1
source=source.replace(needle,r''' if($c.op-eq'launch-picker'){
  if($c.mode-notin@('open','save','folder')){throw 'Unknown owned picker mode'}
  $hex=-join($fixture+'\source').ToCharArray().ForEach({([int]$_).ToString('x4')})
  $request=@{mode=$c.mode;title='Ottrin chooser QA';initial_directory=@{encoding='windows-utf16-le';units_hex=$hex}}
  [IO.File]::WriteAllText("$caseRoot\picker-request.json",($request|ConvertTo-Json -Depth 5),[Text.UTF8Encoding]::new($false))
  $p=Start-Process $app -ArgumentList @('picker','--renderer','opengl') -WorkingDirectory (Split-Path $app) -PassThru -RedirectStandardInput "$caseRoot\picker-request.json" -RedirectStandardOutput "$root\results\app.stdout" -RedirectStandardError "$root\results\app.stderr"
 }else{
'''+needle+r'''
 }''')
extension=e/'picker-actor.ps1';extension.write_text(source);new_sha=hashlib.sha256(extension.read_bytes()).hexdigest();n.GROOT=r'C:\OttrinWindowQA'
guard=r'''$ErrorActionPreference='Stop';$r='C:\OttrinWindowQA';$owner=Get-Content "$r\actor-config.json" -Raw|ConvertFrom-Json;$u=Get-LocalUser $owner.account;if($owner.account-ne'OttrinWindowQA'-or$u.SID.Value-ne$owner.sid-or$u.Description-ne'Ottrin owned packaged workflow QA'){throw 'Wrong QA account'};if((Get-FileHash "$r\actor.ps1").Hash.ToLowerInvariant()-ne'OLD'){throw 'Canonical QA actor changed'};if(Test-Path "$r\actor-picker.next.ps1"){throw 'Unrelated extension exists'}'''.replace('OLD',old_sha)
try:
 n.wait(n.start(guard));n.upload(extension,'actor-picker.next.ps1')
 script=r'''$ErrorActionPreference='Stop';$r='C:\OttrinWindowQA';if((Get-FileHash "$r\actor.ps1").Hash.ToLowerInvariant()-ne'OLD'-or(Get-FileHash "$r\actor-picker.next.ps1").Hash.ToLowerInvariant()-ne'NEW'){throw 'QA actor identity changed'};$tokens=$null;$errors=$null;$null=[Management.Automation.Language.Parser]::ParseFile("$r\actor-picker.next.ps1",[ref]$tokens,[ref]$errors);if($errors.Count){throw ($errors|Out-String)};Move-Item "$r\actor-picker.next.ps1" "$r\actor.ps1" -Force;@{status='pass';canonical_sha256='OLD';extension_sha256='NEW';scope='Owned QA actor only; package and original guest files unchanged'}|ConvertTo-Json'''.replace('OLD',old_sha).replace('NEW',new_sha)
 result=n.wait(n.start(script));proof=json.loads(base64.b64decode(result['out-data']).decode('utf-8-sig'));assert proof['status']=='pass';(g/'PICKER_ACTOR_EXTENSION.json').write_text(json.dumps(proof,indent=2)+'\n')
finally:n.cancel_active()
