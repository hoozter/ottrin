"""Read only the closed owned chooser response, preferences and process state."""
from pathlib import Path
import argparse,json,base64
import native_compiler as n
p=Path(__file__).resolve().parent;g=p.parent/'windows-picker-gui-current';parser=argparse.ArgumentParser();parser.add_argument('--name',choices=['cancel','save','reopen'],required=True);args=parser.parse_args();assert json.loads((g/'SESSION.json').read_text())['status']=='waiting for owned input'
script=r'''$ErrorActionPreference='Stop';$r='C:\OttrinWindowQA';$o=Get-Content "$r\actor-config.json" -Raw|ConvertFrom-Json;$u=Get-LocalUser $o.account;if($o.account-ne'OttrinWindowQA'-or$u.SID.Value-ne$o.sid-or$u.Description-ne'Ottrin owned packaged workflow QA'){throw 'Wrong QA owner'};if(@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath-like"$r\payload\*"}).Count){throw 'Payload still live'};@{status='pass';response=(Get-Content "$r\results\app.stdout" -Raw|ConvertFrom-Json);picker=(Get-Content "$r\cases\ordinary\roaming\ottrin\picker.json" -Raw|ConvertFrom-Json);main_config_sha256=(Get-FileHash "$r\cases\ordinary\roaming\ottrin\config.json").Hash.ToLowerInvariant();payload_processes=0}|ConvertTo-Json -Depth 12'''
try:
 result=n.wait(n.start(script));proof=json.loads(base64.b64decode(result['out-data']).decode('utf-8-sig'));assert proof['status']=='pass';before=json.loads((g/'MAIN-OPENGL_SAVED_PREFERENCES.json').read_text());assert proof['main_config_sha256']==before['config_sha256'];proof['main_config_bytes_unchanged']=True;(g/(args.name.upper()+'_PICKER_STATE.json')).write_text(json.dumps(proof,indent=2)+'\n')
finally:n.cancel_active()
