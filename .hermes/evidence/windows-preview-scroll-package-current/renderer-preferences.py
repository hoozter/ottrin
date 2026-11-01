"""Read only the owned, closed GUI profile and redirected outputs before teardown."""
from pathlib import Path, PureWindowsPath
import argparse, base64, hashlib, json
parser=argparse.ArgumentParser()
parser.add_argument("--name",required=True)
parser.add_argument("--renderer",choices=["Automatic","Native","OpenGlCompatibility"],required=True)
args=parser.parse_args()
assert args.name.isascii() and args.name.replace("-", "").isalnum()
import native_compiler as n
p = Path(__file__).resolve().parent
e = p.parent / 'windows-preview-renderer-gui-current'
session = json.loads((e / 'SESSION.json').read_text())
assert session['status'] == 'waiting for owned input'
close = json.loads((e / 'NORMAL_CLOSE.json').read_text())
assert len(close['normal_app_close']) == 1 and close['normal_app_close'][0]['exit'] == 0
script = r'''$ErrorActionPreference='Stop';$root='C:\OttrinWindowQA';$path="$root\cases\ordinary\roaming\ottrin\config.json";$c=Get-Content $path -Raw|ConvertFrom-Json;$owned=@(Get-CimInstance Win32_Process|Where-Object {$_.ExecutablePath-like"$root\payload\*"});if($owned.Count){throw 'Owned payload remains live'};@{status='pass';theme=$c.theme;default_view_mode=$c.default_view_mode;graphics_renderer=$c.graphics_renderer;config_sha256=(Get-FileHash $path).Hash.ToLowerInvariant();owned_payload_processes=$owned.Count}|ConvertTo-Json'''
try:
    response = n.wait(n.start(script))
finally:
    n.cancel_active()
value = json.loads(base64.b64decode(response['out-data']).decode('utf-8-sig'))
assert value['theme'] == 'Dark' and value['default_view_mode'] == 'Miller'
assert value['graphics_renderer'] == args.renderer and value['owned_payload_processes'] == 0
(e / (args.name.upper()+'_SAVED_PREFERENCES.json')).write_text(json.dumps(value, indent=2) + '\n')
outputs = {}
for name in ['app.stdout', 'app.stderr']:
    handle = n.qga('guest-file-open', {'path': str(PureWindowsPath(r'C:\OttrinWindowQA\results') / name), 'mode': 'rb'})
    data = bytearray()
    try:
        while True:
            block = n.qga('guest-file-read', {'handle': handle, 'count': 65536})
            data.extend(base64.b64decode(block.get('buf-b64', '')))
            assert len(data) < 1024 * 1024
            if block.get('eof'):
                break
    finally:
        n.qga('guest-file-close', {'handle': handle})
    outputs[name] = {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}
    if data:
        (e / (args.name+'-'+name)).write_bytes(data)
(e / (args.name.upper()+'_APP_OUTPUTS.json')).write_text(json.dumps(outputs, indent=2) + '\n')
print('Saved Dark/Column/'+args.renderer+' preferences verified after normal Close; no owned payload processes.')
