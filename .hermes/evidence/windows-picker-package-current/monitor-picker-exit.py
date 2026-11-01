"""Pin the owned chooser process handle before UI cancellation/acceptance."""
from pathlib import Path
import argparse,json,base64
import native_compiler as n
p=Path(__file__).resolve().parent;g=p.parent/'windows-picker-gui-current';parser=argparse.ArgumentParser();parser.add_argument('--name',choices=['cancel','save','reopen'],required=True);args=parser.parse_args();assert json.loads((g/'SESSION.json').read_text())['status']=='waiting for owned input'
script=r'''$ErrorActionPreference='Stop';$r='C:\OttrinWindowQA';$o=Get-Content "$r\actor-config.json" -Raw|ConvertFrom-Json;$u=Get-LocalUser $o.account;if($o.account-ne'OttrinWindowQA'-or$u.SID.Value-ne$o.sid-or$u.Description-ne'Ottrin owned packaged workflow QA'){throw 'Wrong QA owner'};$apps=@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath-eq"$r\payload\Ottrin-0.1.0-windows-x64\bin\ottrin.exe"});if($apps.Count-ne1-or$apps[0].CommandLine-notmatch'\spicker\s'-or(Invoke-CimMethod $apps[0] -MethodName GetOwner).User-ne$o.account){throw 'One owned chooser required'};$p=Get-Process -Id $apps[0].ProcessId;$null=$p.Handle;try{if(-not$p.WaitForExit(120000)){throw 'Read-only chooser exit monitor expired'};if($p.ExitCode-ne0){throw "Chooser exit $($p.ExitCode)"};@{status='pass';pid=$apps[0].ProcessId;exit=$p.ExitCode;owned_handle_pinned_before_exit=$true;scope='Observe actual UI-selected completion; no close or kill issued by this monitor'}|ConvertTo-Json}finally{$p.Dispose()}'''
try:
 result=n.wait(n.start(script),seconds=130);proof=json.loads(base64.b64decode(result['out-data']).decode('utf-8-sig'));assert proof['status']=='pass';(g/(args.name.upper()+'_PICKER_EXIT.json')).write_text(json.dumps(proof,indent=2)+'\n')
finally:n.cancel_active()
